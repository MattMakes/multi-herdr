use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Moved};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::integration::adapters::sqlite::{self as rows, BranchFact};
use crate::fleetd::integration::application::features;
use crate::fleetd::integration::application::landing::{self, BranchRescues};
use crate::fleetd::integration::domain::feature::FeatureState;
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::fleetd::task_execution::application::cancel::{self, Cancel};
use crate::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use crate::fleetd::worktree_custody::application::create::fleet_clock;
use crate::fleetd::worktree_custody::domain::{rescue, rescue_class};
use crate::taskledger::model::Actor;
use crate::taskledger::runs::Run;
use crate::taskledger::{runs, tasks, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abandoned {
    pub branch: String,
    pub rescue_ref: Option<String>,
    pub already: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rescued {
    pub branch: String,
    pub rescue_ref: Option<String>,
}

fn abandonable(conn: &Connection, branch: &str) -> Result<LandingState> {
    let Some(row) = rows::branch(conn, branch)? else {
        bail!("{branch} is not a task branch in the ledger");
    };
    let word = row
        .landing_state
        .with_context(|| format!("{branch} has no landing state"))?;
    let state = landing::state_named(&word)?;
    if state == LandingState::Landed {
        bail!("{branch} is landed; a landed branch is not abandoned");
    }
    Ok(state)
}

pub fn rescue(
    conn: &Connection,
    rescues: &dyn BranchRescues,
    repo: &Path,
    branch: &str,
) -> Result<Rescued> {
    abandonable(conn, branch)?;
    Ok(Rescued {
        branch: branch.to_string(),
        rescue_ref: rescues.rescue(repo, branch)?,
    })
}

pub fn record(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    rescued: &Rescued,
    why: &str,
) -> Result<Abandoned> {
    let branch = rescued.branch.as_str();
    let state = abandonable(tx, branch)?;
    if let Some(name) = rescued.rescue_ref.as_deref() {
        check_ref_name(branch, name)?;
    }
    let row = rows::branch(tx, branch)?.with_context(|| format!("no task branch {branch}"))?;
    if state == LandingState::Abandoned {
        return Ok(Abandoned {
            branch: branch.to_string(),
            rescue_ref: row.rescue_ref,
            already: true,
        });
    }
    let cause = Cause {
        actor: Actor::Horch,
        text: why,
    };
    if let Some(name) = rescued.rescue_ref.as_deref() {
        if row.rescue_ref.as_deref() != Some(name) {
            rows::set_branch_fact(tx, ctx.guard, branch, BranchFact::RescueRef(name), cause)?;
        }
    }
    let clock = fleet_clock(tx)?;
    let moved = landing::enter(ctx, tx, branch, LandingState::Abandoned, clock, why)?;
    if moved == Moved::Unchanged {
        bail!("{branch} did not move to abandoned");
    }
    Ok(Abandoned {
        branch: branch.to_string(),
        rescue_ref: rescued.rescue_ref.clone().or(row.rescue_ref),
        already: false,
    })
}

fn check_ref_name(branch: &str, name: &str) -> Result<()> {
    let id = rescue_class::branch_ref_id(branch)
        .with_context(|| format!("the branch name {branch:?} cannot name a rescue ref folder"))?;
    if !name.starts_with(&rescue::ref_folder(&id)) {
        bail!("{name} is not a rescue ref of {branch}");
    }
    Ok(())
}

pub fn abandon(
    store: &mut Store,
    ctx: &JobCtx<'_>,
    rescues: &dyn BranchRescues,
    repo: &Path,
    branch: &str,
    why: &str,
) -> Result<Abandoned> {
    let rescued = rescue(store.conn(), rescues, repo, branch)?;
    store.immediate(|tx| record(ctx, tx, &rescued, why))
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeatureAbandoned {
    pub feature: String,
    pub branches: Vec<Abandoned>,
    pub cancels: Vec<Cancel>,
    pub already: bool,
}

impl FeatureAbandoned {
    pub fn to_stop(&self) -> Vec<String> {
        let mut out: Vec<String> = self.cancels.iter().flat_map(Cancel::to_stop).collect();
        out.sort();
        out.dedup();
        out
    }
}

fn abandonable_feature(conn: &Connection, feature: &str) -> Result<(FeatureState, Run)> {
    let row = rows::feature(conn, feature)?
        .with_context(|| format!("{feature} is not a feature in the ledger"))?;
    let state = FeatureState::parse(&row.state)
        .with_context(|| format!("{feature} has the state {:?}", row.state))?;
    if state == FeatureState::Merged {
        bail!("{feature} is merged; a merged feature is not abandoned");
    }
    let run = runs::feature_run(conn, feature)?
        .with_context(|| format!("{feature} has no run in the ledger"))?;
    Ok((state, run))
}

pub fn feature_repo(conn: &Connection, feature: &str) -> Result<PathBuf> {
    let (_, run) = abandonable_feature(conn, feature)?;
    Ok(PathBuf::from(run.project_root))
}

fn open_branches_of_feature(conn: &Connection, feature: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM branches WHERE feature_id = ?1 AND task_id IS NOT NULL
           AND landing_state NOT IN ('landed', 'abandoned') ORDER BY name",
    )?;
    let rows = stmt.query_map([feature], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn rescue_feature(
    conn: &Connection,
    rescues: &dyn BranchRescues,
    repo: &Path,
    feature: &str,
) -> Result<Vec<Rescued>> {
    abandonable_feature(conn, feature)?;
    open_branches_of_feature(conn, feature)?
        .iter()
        .map(|branch| rescue(conn, rescues, repo, branch))
        .collect()
}

pub fn record_feature(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    feature: &str,
    rescued: &[Rescued],
    why: &str,
) -> Result<FeatureAbandoned> {
    let (state, run) = abandonable_feature(tx, feature)?;
    let mut branches = Vec::new();
    for name in open_branches_of_feature(tx, feature)? {
        let Some(r) = rescued.iter().find(|r| r.branch == name) else {
            bail!("{name} has no rescue ref yet; run horch feature abandon {feature} again");
        };
        branches.push(record(ctx, tx, r, why)?);
    }
    let cancels = cancel::request_each(tx, &tasks::of_run(tx, &run.run_id)?, why)?;
    let already = state == FeatureState::Abandoned;
    if !already {
        let cause = Cause {
            actor: Actor::Orchestrator,
            text: why,
        };
        let to = FeatureState::Abandoned.as_str();
        transition(
            tx,
            ctx.guard,
            Entity::Feature,
            feature,
            to,
            cause,
            ctx.needs_you,
        )?;
    }
    let repo = Path::new(&run.project_root);
    features::release_gate_tree(ctx, tx, repo, feature, FeatureState::Abandoned)?;
    Ok(FeatureAbandoned {
        feature: feature.to_string(),
        branches,
        cancels,
        already,
    })
}

pub fn abandon_feature(
    store: &mut Store,
    ctx: &JobCtx<'_>,
    rescues: &dyn BranchRescues,
    repo: &Path,
    feature: &str,
    why: &str,
) -> Result<FeatureAbandoned> {
    let rescued = rescue_feature(store.conn(), rescues, repo, feature)?;
    store.immediate(|tx| record_feature(ctx, tx, feature, &rescued, why))
}

pub fn open_branches_of_task(conn: &Connection, task_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM branches WHERE task_id = ?1
           AND landing_state NOT IN ('landed', 'abandoned') ORDER BY name",
    )?;
    let rows = stmt.query_map([task_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn repo_of(conn: &Connection, branch: &str) -> Result<PathBuf> {
    let row = rows::branch(conn, branch)?
        .with_context(|| format!("{branch} is not a task branch in the ledger"))?;
    let task = row
        .task_id
        .with_context(|| format!("{branch} has no task"))?;
    let run = runs::get(conn, &tasks::get(conn, &task)?.run_id)?;
    Ok(PathBuf::from(run.project_root))
}

pub trait RefTips {
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>>;
}

pub struct GitRefTips {
    pub git: CliWorktreeGit,
}

impl RefTips for GitRefTips {
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>> {
        let spec = format!("{name}^{{commit}}");
        let out = self.git.checked(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &spec,
            ],
        )?;
        if out.status != 0 {
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        ))
    }
}

pub fn verify(refs: &dyn RefTips, repo: &Path, rescued: &Rescued, tip: Option<&str>) -> Result<()> {
    let branch = rescued.branch.as_str();
    let now = refs.tip(repo, &format!("refs/heads/{branch}"))?;
    if now.as_deref() != tip {
        bail!(
            "the tip of {branch} is {}, not {}",
            now.as_deref().unwrap_or("absent"),
            tip.unwrap_or("absent")
        );
    }
    match (rescued.rescue_ref.as_deref(), tip) {
        (None, None) => Ok(()),
        (None, Some(tip)) => bail!("no rescue ref holds the tip {tip} of {branch}"),
        (Some(name), None) => bail!("{name} names no tip: {branch} has none"),
        (Some(name), Some(tip)) => {
            check_ref_name(branch, name)?;
            match refs.tip(repo, name)? {
                Some(at) if at == tip => Ok(()),
                Some(at) => bail!("the rescue ref {name} is at {at}, not at the tip {tip}"),
                None => bail!("the rescue ref {name} does not exist"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_ref_tips_reads_a_project_below_home() {
        let home = tempfile::tempdir().unwrap();
        let home_path = horch_platform::path::for_child(&home.path().canonicalize().unwrap());
        let repo = home_path.join("projects").join("app");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let o = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t.invalid"])
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        git(&["init", "--quiet", "--initial-branch=main"]);
        git(&["commit", "--quiet", "--allow-empty", "-m", "one"]);
        let head = git(&["rev-parse", "HEAD"]);
        let fleet = tempfile::tempdir().unwrap();
        let refs = GitRefTips {
            git: CliWorktreeGit::new("git", fleet.path()).with_home(&home_path),
        };
        let tip = refs.tip(&repo, "refs/heads/main").unwrap();
        assert_eq!(tip.as_deref(), Some(head.as_str()));
    }
}
