//! `ParkIdea` and `CancelIdea`: `horch idea park` and `horch idea cancel`,
//! and the policy on the event `feature-abandoned`
//! (`docs/specs/fleetd/design.md`, "The idea state machine", rows `parked`
//! and `cancelled`; "The feature state machine", row `abandoned`; "A
//! cancel"; A34; `docs/tickets/fleetd-p6/MAP.md`, Q10).
//!
//! No work is lost on a park or a cancel:
//!
//! - [`park`]: in 1 transaction, each worktree of the idea that holds work
//!   and is not `active` gets its rescue first, then the idea moves to
//!   `parked`. A `created` tree moves to `orphaned`, the state of a tree
//!   whose rescue comes first, so dispatch holds its task until the rescue
//!   is verified. Its running tasks go on. No new task of a parked idea is
//!   dispatched (the hold `IdeaParked` of task dispatch). The stop of the
//!   sub-orchestrator process is the rule of the live owner of a feature
//!   (ticket 28): a parked idea needs no process.
//! - [`cancel`]: 1 event of the idea records the cancel request. Each task
//!   of the idea that is not terminal gets the cancel of a task
//!   (`task_execution::application::cancel::request`): a task with a live
//!   attempt waits for the stop of its process, each other task is
//!   `cancelled` at once. The idea moves to `cancelled` when each of its
//!   tasks is terminal. The caller stops each worker in
//!   [`IdeaCancel::stop`] after the commit.
//! - [`complete_cancels`]: the pass. It cancels the open tasks again for
//!   each idea whose cancel waits, so a task that a replan adds is
//!   cancelled too, and it moves the idea when each task is terminal.
//! - [`on_feature_abandoned`]: the policy. Each idea of the feature that is
//!   not `merged` is cancelled. A cancelled idea is not changed, so the
//!   event can be handled again with the same effect.

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::adapters::sqlite::has_fact;
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey};
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause, DoorError};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use crate::fleetd::task_execution::adapters::sqlite as task_rows;
use crate::fleetd::task_execution::application::cancel::{self, Cancel};
use crate::fleetd::worktree_custody::adapters::rescue_git::rescue_key;
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, WorktreeRow, WorktreeState,
};
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::tasks;

/// The state of a parked idea: dispatch holds each ready task of it.
pub const PARKED: &str = "parked";
const CANCELLED: &str = "cancelled";
const MERGED: &str = "merged";

/// The cause of the event that records the cancel request of an idea. Its
/// data holds the reason.
const CANCEL_REQUESTED: &str = "idea cancel requested";

/// The reason of a cancel that the pass completes.
const PASS_REASON: &str = "its idea is cancelled";

/// The tree states that hold work that is not saved, other than `active`.
/// A `reserved` row has no folder yet, a `landing` tree is in the landing
/// state machine of its branch, and `landed`, `rescued` and `removed` hold
/// no unsaved work.
const TO_RESCUE: [WorktreeState; 2] = [WorktreeState::Created, WorktreeState::Orphaned];

/// What [`cancel`] did with 1 idea.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdeaCancel {
    /// The idea is `cancelled`.
    pub cancelled: bool,
    /// The workers that the caller stops after the commit. A task of such
    /// a worker moves to `cancelled` when the pass sees its process gone.
    pub stop: Vec<String>,
}

/// The idea `id`, or an error that names it.
fn idea(conn: &Connection, id: &str) -> Result<IdeaRow> {
    ideas_store::idea(conn, id)?.with_context(|| format!("no idea {id} in the ledger"))
}

/// The refusal of the move of `idea` to `to`, as the door words it. Nothing
/// is written.
fn refused(idea: &IdeaRow, to: &str, reason: &str) -> anyhow::Error {
    DoorError::Refused {
        entity: Entity::Idea,
        id: idea.id.clone(),
        from: idea.state.clone(),
        to: to.to_string(),
        cause: reason.to_string(),
        actor: Actor::Operator,
    }
    .into()
}

/// `horch idea park` of the idea `idea_id` for `reason`, in the open
/// transaction `tx`. Gives the ids of the trees whose rescue it queued, by
/// id. A parked idea is refused: `horch idea approve` or an answer resumes
/// it. A `merged` or `cancelled` idea is refused by the door.
pub fn park(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    idea_id: &str,
    reason: &str,
    clock: FleetSeconds,
) -> Result<Vec<String>> {
    let row = idea(tx, idea_id)?;
    if row.state == PARKED {
        bail!(
            "idea {idea_id} is parked already; `horch idea approve {idea_id}` or an answer resumes it"
        );
    }
    if row.state == MERGED || row.state == CANCELLED {
        return Err(refused(&row, PARKED, reason));
    }
    let mut rescued = Vec::new();
    for tree in trees_to_rescue(tx, idea_id)? {
        rescue_first(ctx, tx, &tree, idea_id, clock)?;
        rescued.push(tree.worktree_id);
    }
    let why = format!("idea parked: {reason}");
    let cause = Cause {
        actor: Actor::Operator,
        text: &why,
    };
    transition(
        tx,
        ctx.guard,
        Entity::Idea,
        idea_id,
        PARKED,
        cause,
        ctx.needs_you,
    )?;
    Ok(rescued)
}

/// The trees of the tasks of the idea `idea_id` in [`TO_RESCUE`], by id.
/// A worktree row has no idea: the task of the row names it.
fn trees_to_rescue(conn: &Connection, idea_id: &str) -> Result<Vec<WorktreeRow>> {
    let mut out = Vec::new();
    for task_id in task_rows::tasks_of_idea(conn, idea_id)? {
        if let Some(row) = trees::of_task(conn, &task_id)? {
            if TO_RESCUE.contains(&row.state) {
                out.push(row);
            }
        }
    }
    out.sort_by(|a, b| a.worktree_id.cmp(&b.worktree_id));
    Ok(out)
}

/// The rescue of `tree` before the park of its idea: a `created` tree moves
/// to `orphaned`, and the `rescue` run of its HEAD is queued. A tree with
/// no HEAD yet gets its run from the reconciler, which queues the rescue of
/// each `orphaned` tree when the `worktree_list` result gives its HEAD. A
/// run of the same HEAD that `failed` is not run again: its needs-you item
/// is open.
fn rescue_first(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    tree: &WorktreeRow,
    idea_id: &str,
    clock: FleetSeconds,
) -> Result<()> {
    let id = tree.worktree_id.as_str();
    if tree.state == WorktreeState::Created {
        let why = format!("its idea {idea_id} is parked: its rescue comes first");
        let cause = Cause {
            actor: Actor::Operator,
            text: &why,
        };
        let orphaned = WorktreeState::Orphaned.as_str();
        transition(
            tx,
            ctx.guard,
            Entity::Worktree,
            id,
            orphaned,
            cause,
            ctx.needs_you,
        )?;
    }
    let Some(head) = tree.head_sha.as_deref() else {
        return Ok(());
    };
    let input_key = rescue_key(&tree.path, head);
    let last = jobs_store::latest(tx, Kind::Rescue, id, &input_key)?;
    if last.is_some_and(|j| j.state == State::Failed) {
        return Ok(());
    }
    let key = JobKey {
        kind: Kind::Rescue,
        subject: id,
        input_key: &input_key,
    };
    jobs::queue(ctx, tx, &key, Some(&tree.repo_root), clock)?;
    Ok(())
}

/// `horch idea cancel` of the idea `idea_id` for `reason`, in the open
/// transaction `tx`. A `cancelled` idea does not change. A `merged` idea is
/// refused by the door.
pub fn cancel(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    idea_id: &str,
    reason: &str,
) -> Result<IdeaCancel> {
    let row = idea(tx, idea_id)?;
    if row.state == CANCELLED {
        return Ok(IdeaCancel {
            cancelled: true,
            stop: Vec::new(),
        });
    }
    if row.state == MERGED {
        return Err(refused(&row, CANCELLED, reason));
    }
    if !has_fact(
        tx,
        Entity::Idea,
        idea_id,
        EventKind::Cancelled,
        CANCEL_REQUESTED,
    )? {
        let cause = Cause {
            actor: Actor::Operator,
            text: CANCEL_REQUESTED,
        };
        let data = json!({ "reason": reason });
        record(
            tx,
            ctx.guard,
            EventKind::Cancelled,
            Entity::Idea,
            idea_id,
            cause,
            &data,
        )?;
    }
    cancel_tasks(ctx, tx, idea_id, reason)
}

/// The cancel of each task of the idea `idea_id` that is not terminal, for
/// `reason`; then, when each task is terminal, the move of the idea to
/// `cancelled`.
fn cancel_tasks(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    idea_id: &str,
    reason: &str,
) -> Result<IdeaCancel> {
    let mut stop = Vec::new();
    for task_id in task_rows::open_tasks_of_idea(tx, idea_id)? {
        let task = tasks::get(tx, &task_id)?;
        match cancel::request(tx, &task, reason)? {
            Cancel::Cancelled { holder, .. } => stop.extend(holder),
            Cancel::Stopping { held, .. } => stop.extend(held),
        }
    }
    stop.sort();
    stop.dedup();
    if !task_rows::open_tasks_of_idea(tx, idea_id)?.is_empty() {
        return Ok(IdeaCancel {
            cancelled: false,
            stop,
        });
    }
    let why = format!("idea cancelled: {reason}");
    let cause = Cause {
        actor: Actor::Operator,
        text: &why,
    };
    transition(
        tx,
        ctx.guard,
        Entity::Idea,
        idea_id,
        CANCELLED,
        cause,
        ctx.needs_you,
    )?;
    Ok(IdeaCancel {
        cancelled: true,
        stop,
    })
}

/// The ideas that are not `merged` and not `cancelled`, by id: each one,
/// or only the ideas of the feature `feature`.
fn live_ideas(conn: &Connection, feature: Option<&str>) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM ideas WHERE state NOT IN (?1, ?2)
            AND (?3 IS NULL OR feature_id = ?3)
         ORDER BY id",
    )?;
    let rows = stmt.query_map(params![MERGED, CANCELLED, feature], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The pass: the cancel of the tasks again for each idea whose cancel
/// waits, by id. Gives what each did. A task of such an idea moves to
/// `cancelled` in the pass (`task_execution::application::cancel::
/// complete_cancels`) when its process is gone; then its idea moves here.
pub fn complete_cancels(ctx: &JobCtx<'_>, tx: &Connection) -> Result<Vec<(String, IdeaCancel)>> {
    let mut out = Vec::new();
    for id in live_ideas(tx, None)? {
        if has_fact(
            tx,
            Entity::Idea,
            &id,
            EventKind::Cancelled,
            CANCEL_REQUESTED,
        )? {
            let done = cancel_tasks(ctx, tx, &id, PASS_REASON)?;
            out.push((id, done));
        }
    }
    Ok(out)
}

/// The policy on the event `feature-abandoned` of the feature `feature_id`:
/// [`cancel`] of each idea of the feature that is not `merged` and not
/// `cancelled`, by id (A34). Gives what each cancel did.
pub fn on_feature_abandoned(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    feature_id: &str,
) -> Result<Vec<(String, IdeaCancel)>> {
    let ids = live_ideas(tx, Some(feature_id))?;
    let reason = format!("its feature {feature_id} is abandoned");
    let mut out = Vec::new();
    for id in ids {
        let done = cancel(ctx, tx, &id, &reason)?;
        out.push((id, done));
    }
    Ok(out)
}
