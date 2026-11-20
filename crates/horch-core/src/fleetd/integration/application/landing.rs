//! The use cases of the landing state machine (ticket 25).
//!
//! `docs/specs/fleetd/design.md`, "Landing a task branch on its feature
//! branch" (the state table; F9), "A cancelled task does not block its
//! idea", "What a release does to a run in flight" (row Phase 4):
//! - [`bind_branch`]: the bind of the first attempt of a task writes the
//!   row of its task branch in `building`, before the branch exists.
//! - [`follow`]: the pass part. Each task branch that is not terminal
//!   follows its task through [`enter`], at most `budget` rows for each
//!   pass. A branch past the time limit of its state opens 1 needs-you item,
//!   and the item ends when the branch is no longer past it.
//! - A cancelled task's branch: [`follow`] gives the [`Effect`] of its
//!   rescue ref, [`run`] writes the ref after the commit, and [`observe`]
//!   records it and moves the branch to `abandoned`. Each step reads the
//!   rows again, so a crash at any point repeats the effect, and the
//!   create-only rescue ref is written 1 time.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::fleetd::fleet_runtime::adapters::cursors_store;
use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError, Moved};
use crate::fleetd::fleet_runtime::domain::budget::RowKind;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, BranchRow, NewBranch,
};
use crate::fleetd::integration::domain::task_branch::{
    self as rules, LandingState, Limits, Next, TaskFact,
};
use crate::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use crate::fleetd::worktree_custody::adapters::rescue_refs_git::write_branch_rescue;
use crate::taskledger::model::{Actor, TaskStatus};
use crate::taskledger::{runs, tasks, util};

/// The kind of the needs-you item of a task branch past its time limit.
pub const OVERDUE_KIND: &str = "landing_overdue";

/// The kind of the needs-you item of a task branch that [`follow`] cannot
/// follow: its rows roll back, and the other branches of the pass go on.
pub const FOLLOW_FAILED_KIND: &str = "landing_follow_failed";

/// The pass cursor of [`follow`].
pub const CURSOR: &str = RowKind::Branches.as_str();

/// The key of the needs-you item of the branch `branch` past its limit.
pub fn overdue_key(branch: &str) -> String {
    format!("{OVERDUE_KIND}:{branch}")
}

/// The rescue ref of a branch tip, written after the commit.
pub trait BranchRescues {
    /// Write the rescue ref of the tip of `branch` in `repo` and give its
    /// name, or `None` when git has no such branch. A second call for the
    /// same tip gives the same ref.
    fn rescue(&self, repo: &Path, branch: &str) -> Result<Option<String>>;
}

/// [`BranchRescues`] through the git of fleetd: no hook, in the sandbox.
pub struct GitBranchRescues<'a> {
    pub git: &'a CliWorktreeGit,
}

impl BranchRescues for GitBranchRescues<'_> {
    fn rescue(&self, repo: &Path, branch: &str) -> Result<Option<String>> {
        let spec = format!("refs/heads/{branch}^{{commit}}");
        let out = self
            .git
            .checked(repo, &["rev-parse", "--verify", "--quiet", &spec])?;
        if out.status != 0 {
            return Ok(None);
        }
        let tip = String::from_utf8_lossy(&out.stdout).trim().to_string();
        write_branch_rescue(self.git, repo, branch, &tip).map(Some)
    }
}

/// What [`follow`] reads with.
pub struct Landing<'a> {
    pub jobs: &'a JobCtx<'a>,
}

/// An effect that runs after the commit of the pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Write the rescue ref of the cancelled task's branch `branch` in the
    /// repository `repo`.
    Rescue { branch: String, repo: PathBuf },
}

/// What an [`Effect`] did, for [`observe`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// The rescue ref of `branch`, or `None` when git had no such branch.
    Rescued {
        branch: String,
        rescue_ref: Option<String>,
    },
}

/// The time limits of the landing states in `config`.
pub fn limits(config: &FleetConfig) -> Limits {
    let jobs = &config.job_limit_s;
    Limits {
        ready_s: config.landing_ready_limit_s,
        repair_s: config.landing_repair_limit_s,
        in_bundle_s: jobs
            .bundle_build
            .saturating_add(jobs.fast_gate)
            .saturating_add(config.landing_job_margin_s),
    }
}

/// The landing state of the word `word`.
pub fn state_named(word: &str) -> Result<LandingState> {
    LandingState::parse(word).with_context(|| format!("{word:?} is not a landing state"))
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// Write the row of the task branch `branch` of `task_id` in `building`,
/// in `tx`, when the bind of an attempt names it and it has no row. The
/// feature is the feature whose branch is the run branch of the task (MAP.md
/// of fleetd-p4, Q5). `false` when the row exists, or when the run has no
/// feature: then the task keeps today's land path.
pub fn bind_branch(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    task_id: &str,
    branch: &str,
    clock: FleetSeconds,
) -> Result<bool> {
    if rows::branch(tx, branch)?.is_some() {
        return Ok(false);
    }
    let task = tasks::get(tx, task_id)?;
    let Some(run_branch) = runs::get(tx, &task.run_id)?.run_branch else {
        return Ok(false);
    };
    let Some(feature_id) = live_feature_of(tx, &run_branch)? else {
        return Ok(false);
    };
    let new = NewBranch {
        name: branch,
        feature_id: &feature_id,
        task_id: Some(task_id),
        owner: rules::owner(LandingState::Building),
        // `building` has the limits of its task, so this is its entry time.
        due_at: clock.0,
    };
    let text = format!("the bind of {} names its task branch", task.short());
    rows::insert_branch(tx, ctx.guard, &new, by_fleetd(&text))?;
    Ok(true)
}

/// The `open` or `ready` feature whose branch is `branch`.
fn live_feature_of(conn: &Connection, branch: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT id FROM features WHERE branch = ?1 AND state IN ('open', 'ready')
             ORDER BY id DESC LIMIT 1",
            [branch],
            |r| r.get(0),
        )
        .optional()?)
}

/// Move the task branch `branch` to `to` through `transition()`, in `tx`,
/// and write the owner and the due time of `to` at the fleet clock `clock`.
/// A refused move is a [`DoorError::Refused`], recorded by the door.
pub fn enter(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    branch: &str,
    to: LandingState,
    clock: FleetSeconds,
    why: &str,
) -> Result<Moved> {
    let moved = transition(
        tx,
        ctx.guard,
        Entity::TaskBranch,
        branch,
        to.as_str(),
        by_fleetd(why),
        ctx.needs_you,
    )?;
    if moved == Moved::Unchanged {
        return Ok(moved);
    }
    let row = rows::branch(tx, branch)?.with_context(|| format!("no task branch {branch}"))?;
    let owner = rules::owner(to);
    if row.owner != owner {
        rows::set_branch_fact(
            tx,
            ctx.guard,
            branch,
            BranchFact::Owner(owner),
            by_fleetd(why),
        )?;
    }
    let limits = limits(ctx.config);
    if let Some(due) = rules::due_at(to, clock.0, &limits) {
        rows::set_branch_fact(
            tx,
            ctx.guard,
            branch,
            BranchFact::DueAt(due),
            by_fleetd(why),
        )?;
    }
    Ok(moved)
}

/// The pass part of the landing, in the transaction `tx` of part 2 at the
/// fleet clock `clock`: at most `budget` task branches that are not
/// terminal, in the turn order of the cursor [`CURSOR`]. Each branch
/// follows its task; each branch past its limit has 1 open needs-you item.
/// Gives the effects to run after the commit.
pub fn follow(
    l: &Landing<'_>,
    tx: &Transaction<'_>,
    clock: FleetSeconds,
    budget: usize,
) -> Result<Vec<Effect>> {
    let now = util::now();
    let mut cursor = cursors_store::load(tx, CURSOR, &now)?;
    let names = open_branches(tx)?;
    let mut effects = Vec::new();
    for name in cursor
        .turns(&names, String::as_str)
        .into_iter()
        .take(budget)
    {
        cursor.step(name, &now);
        tx.execute_batch("SAVEPOINT landing_branch")?;
        match follow_one(l, tx, clock, name) {
            Ok(effect) => {
                tx.execute_batch("RELEASE landing_branch")?;
                l.jobs
                    .needs_you
                    .resolve_item(tx, &follow_failed_key(name))?;
                effects.extend(effect);
            }
            Err(e) => {
                tx.execute_batch("ROLLBACK TO landing_branch; RELEASE landing_branch")?;
                open_follow_failed(l.jobs, tx, name, &e)?;
            }
        }
    }
    cursors_store::save(tx, CURSOR, &cursor)?;
    end_items(l.jobs, tx, clock)?;
    end_follow_failed(l.jobs, tx, &names)?;
    Ok(effects)
}

fn follow_failed_key(branch: &str) -> String {
    format!("{FOLLOW_FAILED_KIND}:{branch}")
}

fn open_follow_failed(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    branch: &str,
    e: &anyhow::Error,
) -> Result<()> {
    let text = format!(
        "fleetd cannot follow the task branch {branch}: {e:#}. fleetd tries again on each \
         pass. Run `horch branch abandon {branch}` to end the branch."
    );
    ctx.needs_you.open_item(
        tx,
        &NewItem {
            key: &follow_failed_key(branch),
            kind: FOLLOW_FAILED_KIND,
            entity: Entity::TaskBranch,
            entity_id: branch,
            text: &text,
        },
    )
}

fn end_follow_failed(ctx: &JobCtx<'_>, tx: &Connection, open: &[String]) -> Result<()> {
    for key in needs_you::open_keys(tx, FOLLOW_FAILED_KIND)? {
        let Some(branch) = key.strip_prefix(&format!("{FOLLOW_FAILED_KIND}:")) else {
            continue;
        };
        if !open.iter().any(|n| n == branch) {
            ctx.needs_you.resolve_item(tx, &key)?;
        }
    }
    Ok(())
}

/// The task branches that are not terminal, by name.
fn open_branches(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM branches WHERE task_id IS NOT NULL
           AND landing_state NOT IN ('landed', 'abandoned') ORDER BY name",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn follow_one(
    l: &Landing<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    name: &str,
) -> Result<Option<Effect>> {
    let row = rows::branch(tx, name)?.with_context(|| format!("no task branch {name}"))?;
    let (Some(task_id), Some(word)) = (row.task_id.as_deref(), row.landing_state.as_deref()) else {
        return Ok(None);
    };
    let state = state_named(word)?;
    let task = tasks::get(tx, task_id)?;
    match rules::next(state, task_fact(&task.status)) {
        Next::Stay => {}
        Next::Moves(moves) => {
            let why = format!("its task {} is {}", task.short(), task.status);
            for to in moves {
                match enter(l.jobs, tx, name, to, clock, &why) {
                    Ok(_) => {}
                    // The door recorded the refusal and its needs-you item.
                    Err(e) if matches!(e.downcast_ref(), Some(DoorError::Refused { .. })) => break,
                    Err(e) => return Err(e),
                }
            }
        }
        Next::Rescue => {
            let repo = PathBuf::from(runs::get(tx, &task.run_id)?.project_root);
            return Ok(Some(Effect::Rescue {
                branch: name.to_string(),
                repo,
            }));
        }
    }
    let row = rows::branch(tx, name)?.with_context(|| format!("no task branch {name}"))?;
    open_if_overdue(l.jobs, tx, clock, &row)?;
    Ok(None)
}

/// The landing fact of the task status word `status`.
fn task_fact(status: &str) -> TaskFact {
    let is = |s: TaskStatus| status == s.as_str();
    if is(TaskStatus::Verifying) {
        TaskFact::Verifying
    } else if is(TaskStatus::Done) {
        TaskFact::Done
    } else if is(TaskStatus::Cancelled) {
        TaskFact::Cancelled
    } else if [
        TaskStatus::Leased,
        TaskStatus::Running,
        TaskStatus::Stalled,
        TaskStatus::BlockedOnUser,
        TaskStatus::WaitingQuota,
    ]
    .into_iter()
    .any(is)
    {
        TaskFact::Working
    } else {
        TaskFact::Between
    }
}

/// Whether `row` is past the limit of its state at `clock`.
fn is_overdue(ctx: &JobCtx<'_>, row: &BranchRow, clock: FleetSeconds) -> bool {
    let state = row.landing_state.as_deref().and_then(LandingState::parse);
    state.is_some_and(|s| rules::overdue(s, row.due_at, clock.0, &limits(ctx.config)))
}

/// Open the needs-you item of `row` when it is past its limit. An open
/// item with the same text does not change.
fn open_if_overdue(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    row: &BranchRow,
) -> Result<()> {
    if !is_overdue(ctx, row, clock) {
        return Ok(());
    }
    let state = row.landing_state.as_deref().unwrap_or_default();
    let key = overdue_key(&row.name);
    let text = format!(
        "The task branch {} is {state} past its time limit. Its owner is {}. Read its events.",
        row.name, row.owner
    );
    ctx.needs_you.open_item(
        tx,
        &NewItem {
            key: &key,
            kind: OVERDUE_KIND,
            entity: Entity::TaskBranch,
            entity_id: &row.name,
            text: &text,
        },
    )
}

/// End each open item of [`OVERDUE_KIND`] whose branch is no longer past
/// its limit: it moved on, it is terminal, or its row is gone.
fn end_items(ctx: &JobCtx<'_>, tx: &Connection, clock: FleetSeconds) -> Result<()> {
    for key in needs_you::open_keys(tx, OVERDUE_KIND)? {
        let Some(branch) = key.strip_prefix(&format!("{OVERDUE_KIND}:")) else {
            continue;
        };
        let still = rows::branch(tx, branch)?.is_some_and(|row| is_overdue(ctx, &row, clock));
        if !still {
            ctx.needs_you.resolve_item(tx, &key)?;
        }
    }
    Ok(())
}

/// Run `effect` after the commit of its pass.
pub fn run(effect: &Effect, rescues: &dyn BranchRescues) -> Result<Done> {
    match effect {
        Effect::Rescue { branch, repo } => Ok(Done::Rescued {
            branch: branch.clone(),
            rescue_ref: rescues.rescue(repo, branch)?,
        }),
    }
}

/// Record what an effect did, in `tx`. A rescued branch whose task is still
/// cancelled names its rescue ref and moves to `abandoned`. A branch that
/// moved on meanwhile does not change.
pub fn observe(ctx: &JobCtx<'_>, tx: &Connection, clock: FleetSeconds, done: &Done) -> Result<()> {
    let Done::Rescued { branch, rescue_ref } = done;
    let Some(row) = rows::branch(tx, branch)? else {
        return Ok(());
    };
    let (Some(task_id), Some(word)) = (row.task_id.as_deref(), row.landing_state.as_deref()) else {
        return Ok(());
    };
    if state_named(word)?.is_terminal()
        || tasks::get(tx, task_id)?.status != TaskStatus::Cancelled.as_str()
    {
        return Ok(());
    }
    let why = "its task is cancelled, and the rescue ref of its tip is written";
    if let Some(name) = rescue_ref {
        if row.rescue_ref.as_deref() != Some(name.as_str()) {
            rows::set_branch_fact(
                tx,
                ctx.guard,
                branch,
                BranchFact::RescueRef(name),
                by_fleetd(why),
            )?;
        }
    }
    enter(ctx, tx, branch, LandingState::Abandoned, clock, why)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_words_give_their_landing_fact() {
        assert_eq!(task_fact("verifying"), TaskFact::Verifying);
        assert_eq!(task_fact("done"), TaskFact::Done);
        assert_eq!(task_fact("cancelled"), TaskFact::Cancelled);
        for w in [
            "leased",
            "running",
            "stalled",
            "blocked_on_user",
            "waiting_quota",
        ] {
            assert_eq!(task_fact(w), TaskFact::Working, "{w}");
        }
        for w in ["pending", "ready", "failed", "escalated"] {
            assert_eq!(task_fact(w), TaskFact::Between, "{w}");
        }
    }

    #[test]
    fn in_bundle_limit_is_the_2_jobs_plus_the_margin() {
        let c = FleetConfig::default();
        let l = limits(&c);
        assert_eq!(l.ready_s, 1800);
        assert_eq!(l.repair_s, 3600);
        assert_eq!(
            l.in_bundle_s,
            c.job_limit_s.bundle_build + c.job_limit_s.fast_gate + 600
        );
    }

    #[test]
    fn overdue_key_names_the_branch() {
        assert_eq!(overdue_key("fleet/r1-T1"), "landing_overdue:fleet/r1-T1");
    }
}
