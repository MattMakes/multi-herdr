//! `horch branch abandon` (ticket 35) and `horch feature abandon` (ticket
//! 40). The application layer of integration: use cases over the domain.
//!
//! `docs/specs/fleetd/design.md`, "Landing a task branch on its feature
//! branch": `horch branch abandon <branch>` is the other exit of
//! `conflicted` and `gate_failed`. fleetd writes a rescue ref of the branch,
//! then moves it to `abandoned` (I10). The task stays as it is.
//!
//! [`abandon`] is crash safe: [`rescue`] writes the create-only rescue ref
//! of the tip and reads it back, and only then [`record`] commits the ref
//! name and the move in 1 transaction. A crash before the commit leaves the
//! branch in its state with its rescue ref. The next [`abandon`] gets the
//! same ref for the same tip, so it writes no second ref and commits 1
//! move. The cancel of a task uses the same 2 steps for its task branch.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::application::ledger::Moved;
use crate::fleetd::integration::adapters::sqlite::{self as rows, BranchFact};
use crate::fleetd::integration::application::landing::{self, BranchRescues};
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use crate::fleetd::worktree_custody::application::create::fleet_clock;
use crate::fleetd::worktree_custody::domain::{rescue, rescue_class};
use crate::taskledger::model::Actor;
use crate::taskledger::{runs, tasks, Store};

/// What [`abandon`] or [`record`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abandoned {
    pub branch: String,
    /// The rescue ref of the tip, or `None` when git has no such branch.
    pub rescue_ref: Option<String>,
    /// The branch was `abandoned` before this call: nothing moved.
    pub already: bool,
}

/// The rescue ref of a branch tip, written before the move (I10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rescued {
    pub branch: String,
    /// `None` when git has no such branch: it has no tip to keep.
    pub rescue_ref: Option<String>,
}

/// The landing state of the row of `branch`. A branch with no row, and a
/// `landed` branch, are refused.
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

/// Step 1, before any write of the ledger: write and read back the rescue
/// ref of the tip of `branch` in `repo`. A second call for the same tip
/// gives the same ref.
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

/// Step 2, in `tx`: record the rescue ref of `rescued` on the row and move
/// the branch to `abandoned` through `transition()`, with the cause `why`.
/// The row is read again: a branch that is `landed` now is refused, and a
/// branch that is `abandoned` already is not moved again.
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

/// A rescue ref of `branch` is a ref under its own rescue folder.
fn check_ref_name(branch: &str, name: &str) -> Result<()> {
    let id = rescue_class::branch_ref_id(branch)
        .with_context(|| format!("the branch name {branch:?} cannot name a rescue ref folder"))?;
    if !name.starts_with(&rescue::ref_folder(&id)) {
        bail!("{name} is not a rescue ref of {branch}");
    }
    Ok(())
}

/// Abandon the task branch `branch` of the repository `repo`: [`rescue`],
/// then [`record`] in 1 transaction of `store`.
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

/// The task branches of `task_id` that are not `landed` or `abandoned`.
pub fn open_branches_of_task(conn: &Connection, task_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM branches WHERE task_id = ?1
           AND landing_state NOT IN ('landed', 'abandoned') ORDER BY name",
    )?;
    let rows = stmt.query_map([task_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The repository of the task branch `branch`: the project of the run of
/// its task.
pub fn repo_of(conn: &Connection, branch: &str) -> Result<PathBuf> {
    let row = rows::branch(conn, branch)?
        .with_context(|| format!("{branch} is not a task branch in the ledger"))?;
    let task = row
        .task_id
        .with_context(|| format!("{branch} has no task"))?;
    let run = runs::get(conn, &tasks::get(conn, &task)?.run_id)?;
    Ok(PathBuf::from(run.project_root))
}

/// The commit that a ref names in a repository, read with no write.
pub trait RefTips {
    /// The commit of the full ref name `name` in `repo`, or `None` when
    /// git has no such ref.
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>>;
}

/// [`RefTips`] through the git of fleetd: no hook, in the sandbox.
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

/// Check in git, with no write, that `rescued` holds the tip `tip` of its
/// branch in `repo`: the branch is at `tip`, and the rescue ref exists and
/// names `tip`. With no tip, git has no such branch and no ref is named.
/// The request of fleetd uses it: it does not trust the facts of its
/// payload.
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
