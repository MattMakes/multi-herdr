//! The judge branch in a fix round (ticket 42 of fleetd-p6).
//!
//! `docs/specs/fleetd/design.md`, "The judge pair" (step 6), I9, I10 and
//! RR-3. The judge branch holds only its verdict file on top of the judged
//! branch. When the fix attempt of the judged task passes its structural
//! check, the judge branch must sit on the new judged tip, so the next
//! judge reads the change:
//! - [`follow`] is the pass part. For each judge task in `pending` or
//!   `ready` (no attempt holds it) whose judged task is in `verifying` with
//!   a passed structural check, it reads the tip of the judge branch and
//!   records it as `seen_tip`. When that tip holds the judged tip, nothing
//!   is due. When the rescue ref that the branch row names is not at that
//!   tip, the pass gives [`Effect::Rescue`] (I10). Else it gives
//!   [`Effect::Move`].
//! - [`run`] is the effect after the commit. A rescue writes the
//!   create-only rescue ref of the old tip. A move writes 1 commit with the
//!   verdict file on top of the new judged tip (`git commit-tree`, no
//!   worktree), then moves the judge branch by compare-and-set on
//!   `seen_tip` (I9).
//! - [`observe`] records the rescue ref, or the new tip as `seen_tip`.
//!
//! Each step reads the rows again, so a crash at any point repeats the
//! step: the rescue ref is written 1 time, and the branch moves 1 time. The
//! next attempt of the judge resets its worktree to the moved branch,
//! through custody.

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

/// The identity of each rebuilt judge commit. The global config is not
/// read, so git has no identity of the user.
const IDENTITY: [&str; 4] = [
    "-c",
    "user.name=horch fleetd",
    "-c",
    "user.email=fleetd@horch.invalid",
];

/// The landing states of a judge branch while its judge waits: the branch
/// is not yet `ready`, so no bundle holds it.
const WAITING: [LandingState; 2] = [LandingState::Building, LandingState::Verifying];

/// What [`follow`] works with: the job context of the pass and the git of
/// fleetd.
pub struct JudgeBranches<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
    pub refs: &'a dyn JudgeBranchRefs,
}

/// The git facts and writes of the judge branch that the integration
/// gateway does not give.
pub trait JudgeBranchRefs {
    /// Whether `ancestor` is an ancestor of `tip`, or is `tip`.
    fn contains(&self, repo: &Path, tip: &str, ancestor: &str) -> Result<bool>;

    /// Write the rescue ref of `tip` of the branch `branch`, create-only,
    /// and give its name. When the newest rescue ref of the branch already
    /// names `tip`, give that ref.
    fn rescue(&self, repo: &Path, branch: &str, tip: &str) -> Result<String>;

    /// Write 1 commit on `onto` whose tree is the tree of `onto` with each
    /// file that `from` changed after `base`, as `from` has it, and give
    /// its id. No ref moves, and no worktree or index of the project is
    /// used.
    fn rebuild(&self, repo: &Path, base: &str, from: &str, onto: &str) -> Result<String>;
}

/// [`JudgeBranchRefs`] through the git of fleetd: no hook, in the sandbox.
pub struct GitJudgeBranchRefs {
    git: CliWorktreeGit,
    /// The folder of the private index of a rebuild, outside every project.
    index_folder: PathBuf,
}

impl GitJudgeBranchRefs {
    /// The refs of `git`, an adapter that can write the git folder of the
    /// repository. A rebuild keeps its private index in `index_folder`.
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

/// 1 file of `git diff --raw`: its path, and its mode and object at the
/// new side, or `None` when the new side deletes it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    path: String,
    entry: Option<(String, String)>,
}

/// The changes of the output of `git diff --raw -z --no-renames`: each
/// record is `:<old mode> <new mode> <old oid> <new oid> <status>\0<path>\0`.
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

/// An effect that runs after the commit of the pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Write the rescue ref of `tip` of the judge branch `branch` (I10).
    Rescue {
        repo: PathBuf,
        branch: String,
        tip: String,
    },
    /// Rebuild the judge branch `branch` onto the judged tip `onto`, and
    /// move it from `from`, its `seen_tip`, by compare-and-set (I9). `base`
    /// is the judged tip that `from` holds: the files that `from` changed
    /// after `base` are the files of the judge.
    Move {
        repo: PathBuf,
        branch: String,
        from: String,
        base: String,
        onto: String,
    },
}

/// What [`run`] did, for [`observe`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// The rescue ref of the tip of `branch`.
    Rescued { branch: String, rescue_ref: String },
    /// The branch was at `from`, and git moved it to `tip`.
    Moved { branch: String, tip: String },
    /// The branch was not at `from`, and nothing moved. `actual` is its
    /// tip, or `None` when it does not exist.
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

/// The pass part, in `tx`: record the tip of each judge branch that waits
/// for its judge, and give the rescue or the move of each judge branch that
/// does not hold the newest judged tip.
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

/// Whether the judge branch of the judge task `judge` holds the judged tip
/// of `judged_passes`: the rule by which [`follow`] moves nothing. With
/// no judge branch that waits, no tip of it, or no passed check of the
/// judged task, it holds nothing.
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
    // An attempt of the judge works in its worktree: the branch stays.
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
    // The judged tip that the judge branch holds: the newest passed check
    // before `onto` that is in the judge branch.
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

/// Whether the rescue ref that `row` names is at `tip`.
fn rescued(j: &JudgeBranches<'_>, repo: &Path, row: &BranchRow, tip: &str) -> Result<bool> {
    let Some(name) = row.rescue_ref.as_deref() else {
        return Ok(false);
    };
    Ok(j.git.tip(repo, name)?.as_deref() == Some(tip))
}

/// Write `tip` as the `seen_tip` of `row` when it has another tip.
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

/// Run `effect` after the commit of its pass. A move writes its commit
/// only when the branch is at `from`, and moves the branch by 1
/// compare-and-set from `from` (I9).
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

/// Record `done` on its branch row, in `tx`: the rescue ref, or the tip
/// that git has now as `seen_tip`.
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
