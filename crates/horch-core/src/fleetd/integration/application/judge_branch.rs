use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::integration::adapters::sqlite::{self as rows, BranchFact, BranchRow};
use crate::fleetd::integration::application::ports::{IntegrationGitGateway, RefMove};
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::fleetd::task_execution::adapters::verdict_file;
use crate::fleetd::task_execution::domain::structural;
use crate::fleetd::task_execution::domain::task::judged_task;
use crate::fleetd::worktree_custody::adapters::git::{private_folder, CliWorktreeGit};
use crate::fleetd::worktree_custody::adapters::rescue_git::PrivateIndex;
use crate::fleetd::worktree_custody::adapters::rescue_refs_git::write_branch_rescue;
use crate::taskledger::model::{Actor, TaskStatus};
use crate::taskledger::{runs, tasks};

const IDENTITY: [&str; 4] = [
    "-c",
    "user.name=horch fleetd",
    "-c",
    "user.email=fleetd@horch.invalid",
];

const WAITING: [LandingState; 2] = [LandingState::Building, LandingState::Verifying];

pub struct JudgeBranches<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
    pub refs: &'a dyn JudgeBranchRefs,
}

pub trait JudgeBranchRefs {
    fn contains(&self, repo: &Path, tip: &str, ancestor: &str) -> Result<bool>;

    fn rescue(&self, repo: &Path, branch: &str, tip: &str) -> Result<String>;

    fn rebuild(&self, repo: &Path, base: &str, from: &str, onto: &str) -> Result<String>;
}

pub struct GitJudgeBranchRefs {
    git: CliWorktreeGit,
    index_folder: PathBuf,
}

impl GitJudgeBranchRefs {
    pub fn new(git: CliWorktreeGit, index_folder: PathBuf) -> GitJudgeBranchRefs {
        GitJudgeBranchRefs { git, index_folder }
    }
}

impl JudgeBranchRefs for GitJudgeBranchRefs {
    fn contains(&self, repo: &Path, tip: &str, ancestor: &str) -> Result<bool> {
        let out = self
            .git
            .checked(repo, &["merge-base", "--is-ancestor", ancestor, tip])?;
        match out.status {
            0 => Ok(true),
            1 => Ok(false),
            s => bail!(
                "git merge-base --is-ancestor {ancestor} {tip} failed ({s}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    fn rescue(&self, repo: &Path, branch: &str, tip: &str) -> Result<String> {
        write_branch_rescue(&self.git, repo, branch, tip)
    }

    fn rebuild(&self, repo: &Path, base: &str, from: &str, onto: &str) -> Result<String> {
        let raw = self.git.git(
            repo,
            &[
                "diff",
                "--raw",
                "-z",
                "--no-renames",
                "--no-abbrev",
                "--no-ext-diff",
                "--no-textconv",
                base,
                from,
            ],
        )?;
        let changes = parse_raw(&raw)?;
        private_folder(&self.index_folder)?;
        let index = PrivateIndex::new(self.index_folder.join(format!("judge-{from}.index")))?;
        let git = self.git.with_index(&index.path)?;
        git.git(repo, &["read-tree", onto])?;
        for c in &changes {
            match &c.entry {
                Some((mode, oid)) => git.git(
                    repo,
                    &["update-index", "--add", "--cacheinfo", mode, oid, &c.path],
                )?,
                None => git.git(repo, &["update-index", "--force-remove", "--", &c.path])?,
            };
        }
        let tree = git.git(repo, &["write-tree"])?.trim().to_string();
        let message = format!("fleetd: the judge files of {from} on {onto}");
        let mut args = IDENTITY.to_vec();
        args.extend(["commit-tree", &tree, "-p", onto, "-m", &message]);
        Ok(self.git.git(repo, &args)?.trim().to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    path: String,
    entry: Option<(String, String)>,
}

fn parse_raw(raw: &str) -> Result<Vec<Change>> {
    let mut parts = raw.split('\0').filter(|p| !p.is_empty());
    let mut out = Vec::new();
    while let Some(head) = parts.next() {
        let fields: Vec<&str> = head.trim_start_matches(':').split(' ').collect();
        let [_, mode, _, oid, _] = fields[..] else {
            bail!("git diff --raw printed {head:?}");
        };
        let path = parts
            .next()
            .with_context(|| format!("git diff --raw printed no path after {head:?}"))?;
        let deleted = mode.bytes().all(|b| b == b'0');
        out.push(Change {
            path: path.to_string(),
            entry: (!deleted).then(|| (mode.to_string(), oid.to_string())),
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Rescue {
        repo: PathBuf,
        branch: String,
        tip: String,
    },
    Move {
        repo: PathBuf,
        branch: String,
        from: String,
        base: String,
        onto: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    Rescued {
        branch: String,
        rescue_ref: String,
    },
    Moved {
        branch: String,
        tip: String,
    },
    Mismatch {
        branch: String,
        actual: Option<String>,
    },
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

pub fn follow(j: &JudgeBranches<'_>, tx: &Connection) -> Result<Vec<Effect>> {
    let mut effects = Vec::new();
    for state in WAITING {
        for (branch, task) in rows::task_branches_in(tx, state.as_str())? {
            let effect = follow_one(j, tx, &branch, &task)
                .with_context(|| format!("the judge branch {branch}"))?;
            effects.extend(effect);
        }
    }
    Ok(effects)
}

fn judged_passes(tx: &Connection, judged: &str) -> Result<Option<(String, Vec<String>)>> {
    let verdicts = verdict_file::recorded(tx, judged)?;
    let Some(onto) = verdicts
        .last()
        .filter(|v| structural::is_pass(v))
        .map(|v| v.tip.clone())
    else {
        return Ok(None);
    };
    let passes = verdicts
        .into_iter()
        .filter(structural::is_pass)
        .map(|v| v.tip)
        .collect();
    Ok(Some((onto, passes)))
}

pub fn holds_judged_tip(
    git: &dyn IntegrationGitGateway,
    refs: &dyn JudgeBranchRefs,
    tx: &Connection,
    judge: &tasks::Task,
) -> Result<bool> {
    let Some(judged) = judged_task(&judge.kind, judge.parent_task_id.as_deref()) else {
        return Ok(false);
    };
    let Some((onto, _)) = judged_passes(tx, judged)? else {
        return Ok(false);
    };
    let mut branch = None;
    for state in WAITING {
        branch = branch.or(rows::task_branches_in(tx, state.as_str())?
            .into_iter()
            .find(|(_, task)| *task == judge.task_id)
            .map(|(name, _)| name));
    }
    let Some(branch) = branch else {
        return Ok(false);
    };
    let repo = PathBuf::from(runs::get(tx, &judge.run_id)?.project_root);
    let Some(tip) = git.tip(&repo, &format!("refs/heads/{branch}"))? else {
        return Ok(false);
    };
    refs.contains(&repo, &tip, &onto)
}

fn follow_one(
    j: &JudgeBranches<'_>,
    tx: &Connection,
    branch: &str,
    task_id: &str,
) -> Result<Option<Effect>> {
    let judge = tasks::get(tx, task_id)?;
    let Some(judged_id) = judged_task(&judge.kind, judge.parent_task_id.as_deref()) else {
        return Ok(None);
    };
    let between = [TaskStatus::Pending, TaskStatus::Ready];
    if !between.iter().any(|s| s.as_str() == judge.status) {
        return Ok(None);
    }
    if tasks::get(tx, judged_id)?.status != TaskStatus::Verifying.as_str() {
        return Ok(None);
    }
    let Some((onto, passes)) = judged_passes(tx, judged_id)? else {
        return Ok(None);
    };
    let repo = PathBuf::from(runs::get(tx, &judge.run_id)?.project_root);
    let Some(tip) = j.git.tip(&repo, &format!("refs/heads/{branch}"))? else {
        return Ok(None);
    };
    let row = rows::branch(tx, branch)?.with_context(|| format!("no branch row {branch}"))?;
    record_tip(j.jobs, tx, &row, &tip)?;
    if j.refs.contains(&repo, &tip, &onto)? {
        return Ok(None);
    }
    if !rescued(j, &repo, &row, &tip)? {
        return Ok(Some(Effect::Rescue {
            repo,
            branch: branch.to_string(),
            tip,
        }));
    }
    let mut base = None;
    for t in passes.iter().rev().filter(|t| **t != onto) {
        if j.refs.contains(&repo, &tip, t)? {
            base = Some(t.clone());
            break;
        }
    }
    let Some(base) = base else {
        return Ok(None);
    };
    Ok(Some(Effect::Move {
        repo,
        branch: branch.to_string(),
        from: tip,
        base,
        onto,
    }))
}

fn rescued(j: &JudgeBranches<'_>, repo: &Path, row: &BranchRow, tip: &str) -> Result<bool> {
    let Some(name) = row.rescue_ref.as_deref() else {
        return Ok(false);
    };
    Ok(j.git.tip(repo, name)?.as_deref() == Some(tip))
}

fn record_tip(ctx: &JobCtx<'_>, tx: &Connection, row: &BranchRow, tip: &str) -> Result<()> {
    if row.seen_tip.as_deref() == Some(tip) {
        return Ok(());
    }
    rows::set_branch_fact(
        tx,
        ctx.guard,
        &row.name,
        BranchFact::SeenTip(tip),
        by_fleetd("fleetd read the tip of the judge branch"),
    )
}

pub fn run(
    effect: &Effect,
    git: &dyn IntegrationGitGateway,
    refs: &dyn JudgeBranchRefs,
) -> Result<Done> {
    match effect {
        Effect::Rescue { repo, branch, tip } => Ok(Done::Rescued {
            branch: branch.clone(),
            rescue_ref: refs.rescue(repo, branch, tip)?,
        }),
        Effect::Move {
            repo,
            branch,
            from,
            base,
            onto,
        } => {
            let name = format!("refs/heads/{branch}");
            let mismatch = |actual| Done::Mismatch {
                branch: branch.clone(),
                actual,
            };
            let actual = git.tip(repo, &name)?;
            if actual.as_deref() != Some(from.as_str()) {
                return Ok(mismatch(actual));
            }
            let tip = refs.rebuild(repo, base, from, onto)?;
            Ok(match git.update_ref(repo, &name, &tip, Some(from))? {
                RefMove::Moved => Done::Moved {
                    branch: branch.clone(),
                    tip,
                },
                RefMove::Mismatch { actual } => mismatch(actual),
            })
        }
    }
}

pub fn observe(ctx: &JobCtx<'_>, tx: &Connection, done: &Done) -> Result<()> {
    let (branch, fact, why) = match done {
        Done::Rescued { branch, rescue_ref } => (
            branch,
            BranchFact::RescueRef(rescue_ref),
            format!("the rescue ref {rescue_ref} is written before the move of the judge branch"),
        ),
        Done::Moved { branch, tip } => (
            branch,
            BranchFact::SeenTip(tip),
            format!("fleetd moved the judge branch onto the new judged tip: {tip}"),
        ),
        Done::Mismatch {
            branch,
            actual: Some(tip),
        } => (
            branch,
            BranchFact::SeenTip(tip),
            format!("the judge branch is at {tip}, not at its seen tip: nothing moved"),
        ),
        Done::Mismatch { actual: None, .. } => return Ok(()),
    };
    let Some(row) = rows::branch(tx, branch)? else {
        return Ok(());
    };
    let same = match fact {
        BranchFact::RescueRef(r) => row.rescue_ref.as_deref() == Some(r),
        BranchFact::SeenTip(t) => row.seen_tip.as_deref() == Some(t),
        _ => false,
    };
    if same {
        return Ok(());
    }
    rows::set_branch_fact(tx, ctx.guard, branch, fact, by_fleetd(&why))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raw_diff_gives_each_file_and_its_new_side() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let z = "0".repeat(40);
        let raw =
            format!(":000000 100644 {z} {a} A\0docs/v.md\0:100644 000000 {b} {z} D\0old.md\0");
        assert_eq!(
            parse_raw(&raw).unwrap(),
            vec![
                Change {
                    path: "docs/v.md".into(),
                    entry: Some(("100644".into(), a)),
                },
                Change {
                    path: "old.md".into(),
                    entry: None,
                },
            ]
        );
        assert!(parse_raw("").unwrap().is_empty());
        assert!(parse_raw(":100644 A\0x\0").is_err());
    }
}
