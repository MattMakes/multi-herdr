//! The removal of a worktree: the queue of its `worktree_remove` job and
//! the observation of the run.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" (rows `landed`, `rescued`, `removed`) and "The kinds of job" (row
//! `worktree_remove`); MAP.md of fleetd-p3, Q7 and Q8. The pass and a
//! command call [`queue_remove`] and [`observe`] in 1 transaction, with no
//! git call.

use anyhow::{bail, Result};
use rusqlite::{params, Connection};

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
use crate::fleetd::worktree_custody::adapters::sqlite::{self as store, WorktreeRow};
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
