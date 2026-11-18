//! The removal of a worktree: the queue of its `worktree_remove` job and
//! the observation of the run.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" (rows `landed`, `rescued`, `removed`) and "The kinds of job" (row
//! `worktree_remove`); MAP.md of fleetd-p3, Q7 and Q8. The pass and a
//! command call [`queue_remove`] and [`observe`] in 1 transaction, with no
//! git call. A command calls [`remove_tree`], which runs the jobs with no
//! transaction open.

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};

use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::application::jobs::{
    self, outcome, result_data, JobCtx, JobKey, NewItem, Queued,
};
use crate::fleetd::fleet_runtime::application::ledger::{
    transition, Cause, DoorError, Moved, RecordRefusal,
};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::worktree_custody::adapters::remove_git::{remove_key, Removed};
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as store, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::application::create::{self, RunJob};
use crate::fleetd::worktree_custody::domain::removal;
use crate::taskledger::model::Actor;
use crate::taskledger::util;

/// Queue 1 `worktree_remove` run of the tree of `row`, in `tx`: subject
/// its worktree id, input key its path and `keep_branch`
/// ([`remove_key`]), repository its repository. Only a row that is
/// `landed` or `rescued` in `tx` is queued. Each other state is refused:
/// the inner `Err` holds its reason and `cause`, the reason of the caller,
/// and no run is queued. 1 tree has at most 1 open removal: an open run
/// of the tree is given back, and an open run with the other
/// `keep_branch` is refused.
pub fn queue_remove(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    row: &WorktreeRow,
    cause: &str,
    keep_branch: bool,
    clock: FleetSeconds,
) -> Result<Result<Queued, String>> {
    let id = row.worktree_id.as_str();
    let Some(now) = store::get(tx, id)? else {
        return Ok(Err(format!("worktree {id} is not in the ledger ({cause})")));
    };
    if let Err(why) = removal::removable(id, now.state.as_str()) {
        return Ok(Err(format!("{why} ({cause})")));
    }
    let input_key = remove_key(&now.path, keep_branch);
    let key = JobKey {
        kind: Kind::WorktreeRemove,
        subject: id,
        input_key: &input_key,
    };
    match jobs::queue(ctx, tx, &key, Some(&now.repo_root), clock)? {
        Queued::Active(open) if open.input_key != input_key => Ok(Err(format!(
            "worktree {id} has the worktree_remove job {} in {} with another input; \
             run the command again after it ends ({cause})",
            open.id,
            open.state.as_str()
        ))),
        q => Ok(Ok(q)),
    }
}

/// The newest `worktree_remove` run of the tree `id` at `path`, with
/// either `keep_branch`.
pub fn latest_removal(tx: &Connection, id: &str, path: &str) -> Result<Option<JobRow>> {
    let mut newest: Option<JobRow> = None;
    for keep_branch in [false, true] {
        let key = remove_key(path, keep_branch);
        if let Some(run) = jobs_store::latest(tx, Kind::WorktreeRemove, id, &key)? {
            if newest.as_ref().is_none_or(|n| run.id > n.id) {
                newest = Some(run);
            }
        }
    }
    Ok(newest)
}

/// What [`observe`] did with the row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// The row is `removed`.
    Removed,
    /// The removal failed: the row stays, and 1 needs-you item is open.
    Failed,
    /// The row stays as it was.
    Kept,
}

/// Observe the remove run `job`, in `tx`:
/// - `passed`: the row moves to `removed` from `landed` or `rescued`, with
///   today's `cleaned_at`, and the needs-you item of a failed removal of
///   the tree ends. The door refuses each other state and records it.
/// - `failed`: the row stays, and 1 needs-you item opens with the reason.
/// - `abandoned` (its time limit or a lost process), `queued` or
///   `running`: the row stays. The rule of abandoned runs of jobs gives the
///   next run and, at `abandoned_run_limit`, its needs-you item.
pub fn observe(ctx: &JobCtx<'_>, tx: &Connection, job: &JobRow) -> Result<Observed> {
    if job.kind != Kind::WorktreeRemove {
        bail!(
            "job {} is a {} job, not a worktree_remove job",
            job.id,
            job.kind
        );
    }
    let id = job.subject.as_str();
    match job.state {
        State::Passed => match result_data(job).and_then(|d| serde_json::from_value(d).ok()) {
            Some(r) => removed(ctx, tx, id, &r),
            None => failed(ctx, tx, id, "the passed run gives no removal"),
        },
        State::Failed => {
            let reason = outcome(job).map(|o| o.text).unwrap_or_default();
            failed(ctx, tx, id, &reason)
        }
        State::Queued | State::Running | State::Abandoned => Ok(Observed::Kept),
    }
}

fn removed(ctx: &JobCtx<'_>, tx: &Connection, id: &str, r: &Removed) -> Result<Observed> {
    if store::get(tx, id)?.is_none() {
        return Ok(Observed::Kept);
    }
    let (files, verb) = if r.ignored_files == 1 {
        ("file", "was")
    } else {
        ("files", "were")
    };
    let text = format!(
        "its worktree_remove job removed the tree; {} ignored {files} {verb} deleted with it",
        r.ignored_files
    );
    let cause = Cause {
        actor: Actor::Horch,
        text: &text,
    };
    let refusals: &dyn RecordRefusal = ctx.needs_you;
    match transition(
        tx,
        ctx.guard,
        Entity::Worktree,
        id,
        "removed",
        cause,
        refusals,
    ) {
        Ok(Moved::Changed) => {
            tx.execute(
                "UPDATE worktrees SET cleaned_at = ?2 WHERE worktree_id = ?1",
                params![id, util::now()],
            )?;
        }
        Ok(Moved::Unchanged) => {}
        // The door recorded the refusal with 1 event and 1 needs-you item.
        Err(DoorError::Refused { .. }) => return Ok(Observed::Kept),
        Err(e) => return Err(e.into()),
    }
    ctx.needs_you
        .resolve_item(tx, &removal::needs_you_key(id))?;
    Ok(Observed::Removed)
}

fn failed(ctx: &JobCtx<'_>, tx: &Connection, id: &str, reason: &str) -> Result<Observed> {
    let key = removal::needs_you_key(id);
    let text = format!("The removal of worktree {id} failed, so its tree stays: {reason}");
    let item = NewItem {
        key: &key,
        kind: removal::NEEDS_YOU_KIND,
        entity: Entity::Worktree,
        entity_id: id,
        text: &text,
    };
    ctx.needs_you.open_item(tx, &item)?;
    Ok(Observed::Failed)
}

// ─── the command ────────────────────────────────────────────────────────

/// Remove the tree of the row `id` only through custody (RR-9, FM-27),
/// with the connection `conn` and the job context `ctx` of a command. No
/// transaction is open while a job runs.
/// - A `landed` or `rescued` tree: 1 `worktree_remove` job.
/// - Each other tree: it moves to `orphaned` with no owner (`cause`), 1
///   `rescue` job puts it on a verified rescue ref, then 1
///   `worktree_remove` job removes it.
///
/// The inner `Err` is why custody refused it: the tree stays. With
/// `keep_branch` the job never deletes the branch.
pub fn remove_tree(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    id: &str,
    cause: Cause<'_>,
    keep_branch: bool,
) -> Result<Result<(), String>> {
    let row = match removable_row(ctx, conn, id, cause)? {
        Ok(row) => row,
        Err(why) => return Ok(Err(why)),
    };
    if !matches!(row.state, WorktreeState::Landed | WorktreeState::Rescued) {
        if let Err(e) = create::rescue_now(conn, ctx, jobs, &row) {
            return Ok(Err(format!("{e:#}")));
        }
    }
    remove_now(ctx, conn, jobs, id, cause.text, keep_branch)
}

fn immediate(conn: &mut Connection) -> Result<Transaction<'_>> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .context("taking the task ledger's write lock")
}

/// The row `id`, in a state that a rescue or a removal takes. A `landed`,
/// `landing`, `orphaned` or `rescued` row stays as it is. Each other row
/// moves to `orphaned` with no owner, for `cause`. A move that the door
/// refuses is recorded and keeps the row: the inner `Err` says why.
fn removable_row(
    ctx: &JobCtx<'_>,
    conn: &mut Connection,
    id: &str,
    cause: Cause<'_>,
) -> Result<Result<store::WorktreeRow, String>> {
    let tx = immediate(conn)?;
    let row = store::get(&tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    let keep = [
        WorktreeState::Landed,
        WorktreeState::Landing,
        WorktreeState::Orphaned,
        WorktreeState::Rescued,
    ];
    if keep.contains(&row.state) {
        return Ok(Ok(row));
    }
    let moved = transition(
        &tx,
        None,
        Entity::Worktree,
        id,
        "orphaned",
        cause,
        ctx.needs_you,
    );
    if let Err(e) = moved {
        tx.commit()?;
        return Ok(Err(format!("worktree {id} is {}: {e}", row.state)));
    }
    store::set_owner(&tx, id, None)?;
    let row = store::get(&tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    tx.commit()?;
    Ok(Ok(row))
}

/// Queue the `worktree_remove` job of the row `id`, run it through `jobs`
/// with no transaction open, then observe it in 1 transaction.
fn remove_now(
    ctx: &JobCtx<'_>,
    conn: &mut Connection,
    jobs: &dyn RunJob,
    id: &str,
    cause: &str,
    keep_branch: bool,
) -> Result<Result<(), String>> {
    let tx = immediate(conn)?;
    let clock = create::fleet_clock(&tx)?;
    let row = store::get(&tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    let job = match queue_remove(ctx, &tx, &row, cause, keep_branch, clock)? {
        Ok(Queued::New(job)) => job,
        Ok(Queued::Active(r) | Queued::Reused(r)) => r.id,
        Ok(Queued::Held(r)) => {
            return Ok(Err(format!(
                "the worktree_remove job of {} ended abandoned {} times; run horch job requeue {} after you read why",
                row.path, r.abandoned_runs, r.id
            )))
        }
        Err(why) => return Ok(Err(why)),
    };
    tx.commit()?;
    let ended = jobs.run_job(ctx, conn, job)?;
    let tx = immediate(conn)?;
    let seen = observe(ctx, &tx, &ended)?;
    tx.commit()?;
    Ok(match seen {
        Observed::Removed => Ok(()),
        _ => Err(outcome(&ended)
            .map(|o| o.text)
            .unwrap_or_else(|| format!("its worktree_remove job ended {}", ended.state.as_str()))),
    })
}
