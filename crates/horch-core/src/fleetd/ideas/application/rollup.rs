//! The roll-up of an idea: `in_progress` to `integrating` to `merged`.
//!
//! `docs/specs/fleetd/design.md`, "The idea state machine" (rows
//! `in_progress`, `integrating` and `merged`), "Ideas finish clean", FM-39;
//! `ddd/08-code/ideas/design.md` (`RollUpIdea`, `CloseIdea`).
//!
//! - [`roll_up_idea`] (`RollUpIdea`): each task of the idea that is not
//!   `cancelled` is `done`, and each task branch that is not `abandoned`
//!   is `landed` -> `integrating`.
//! - [`close_idea`] (`CloseIdea`): the feature of the idea is `merged`, and
//!   each worktree of the idea is `removed` or `rescued` -> `merged`.
//!
//! A worktree row has no idea (Q2 of fleetd-p3): the trees of an idea are
//! the rows of its tasks. The reconciler removes the `rescued` trees of a
//! `merged` idea ([`idea_state_of_task`]), and the rescue refs stay.
//! A guard whose fact is unknown does not hold: an idea with no task does
//! not roll up.

use std::collections::BTreeSet;

use anyhow::Result;
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError, Moved};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::integration::adapters::sqlite as integration_rows;
use crate::fleetd::task_execution::adapters::sqlite as task_rows;
use crate::fleetd::worktree_custody::adapters::sqlite::{self as tree_rows, WorktreeState};
use crate::taskledger::model::{Actor, TaskStatus};

const IN_PROGRESS: &str = "in_progress";
const INTEGRATING: &str = "integrating";
const MERGED: &str = "merged";

/// What 1 call of [`roll_up_ideas`] moved, by idea id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The ideas moved to `integrating`.
    pub integrating: Vec<String>,
    /// The ideas moved to `merged`.
    pub merged: Vec<String>,
}

/// `roll_up_ideas(tx)` of a pass: [`roll_up_idea`] of each idea in
/// `in_progress`, then [`close_idea`] of each idea in `integrating`, in
/// `tx`. An idea can take both steps in 1 call.
pub fn roll_up_ideas(ctx: &JobCtx<'_>, tx: &Connection) -> Result<Report> {
    let mut report = Report::default();
    for id in ideas_in(tx, IN_PROGRESS)? {
        if roll_up_idea(ctx, tx, &id)? {
            report.integrating.push(id);
        }
    }
    for id in ideas_in(tx, INTEGRATING)? {
        if close_idea(ctx, tx, &id)? {
            report.merged.push(id);
        }
    }
    Ok(report)
}

/// `RollUpIdea`: move the idea `id` from `in_progress` to `integrating`
/// when each of its tasks that is not `cancelled` is `done`, and each of
/// its task branches that is not `abandoned` is `landed`, in `tx`. A
/// cancelled task does not block its idea, but its branch does until it
/// is `abandoned`: that move comes after its rescue ref. `true` when the
/// idea moved.
pub fn roll_up_idea(ctx: &JobCtx<'_>, tx: &Connection, id: &str) -> Result<bool> {
    if !in_state(tx, id, IN_PROGRESS)? || !work_is_in(tx, id)? {
        return Ok(false);
    }
    moved(
        ctx,
        tx,
        id,
        INTEGRATING,
        "each task is done or cancelled, and each task branch is landed or abandoned",
    )
}

/// `CloseIdea`: move the idea `id` from `integrating` to `merged` when its
/// feature is `merged` and each worktree of the idea is `removed` or
/// `rescued`, in `tx`. `true` when the idea moved.
pub fn close_idea(ctx: &JobCtx<'_>, tx: &Connection, id: &str) -> Result<bool> {
    let Some(idea) = ideas_store::idea(tx, id)? else {
        return Ok(false);
    };
    if idea.state != INTEGRATING {
        return Ok(false);
    }
    let feature_merged =
        integration_rows::feature(tx, &idea.feature_id)?.is_some_and(|f| f.state == MERGED);
    if !feature_merged || !trees_are_saved(tx, id)? {
        return Ok(false);
    }
    moved(
        ctx,
        tx,
        id,
        MERGED,
        "its feature is merged, and each of its worktrees is removed or rescued",
    )
}

/// The state of the idea of the task `task_id`; `None` when the task has no
/// idea. The worktree context reads the idea of a tree through its task.
pub fn idea_state_of_task(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let Some(idea) = task_rows::idea_of_task(conn, task_id)? else {
        return Ok(None);
    };
    Ok(ideas_store::idea(conn, &idea)?.map(|i| i.state))
}

/// Each idea in `state`, by id.
fn ideas_in(conn: &Connection, state: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM ideas WHERE state = ?1 ORDER BY id")?;
    let ids = stmt.query_map([state], |r| r.get(0))?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

fn in_state(conn: &Connection, id: &str, state: &str) -> Result<bool> {
    Ok(ideas_store::idea(conn, id)?.is_some_and(|i| i.state == state))
}

/// Each task of the idea `id` that is not cancelled is done, and each task
/// branch that is not abandoned is landed. `false` for an idea with no task.
fn work_is_in(conn: &Connection, id: &str) -> Result<bool> {
    let tasks = task_rows::task_states_of_idea(conn, id)?;
    if tasks.is_empty() {
        return Ok(false);
    }
    let cancelled = TaskStatus::Cancelled.as_str();
    let done = TaskStatus::Done.as_str();
    if tasks.iter().any(|(_, s)| s != cancelled && s != done) {
        return Ok(false);
    }
    for (task, _) in &tasks {
        let states = integration_rows::landing_states_of_task(conn, task)?;
        if !states
            .iter()
            .all(|s| matches!(s.as_deref(), Some("landed" | "abandoned")))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Each worktree row of a task of the idea `id` is `removed` or `rescued`.
fn trees_are_saved(conn: &Connection, id: &str) -> Result<bool> {
    let tasks: BTreeSet<String> = task_rows::tasks_of_idea(conn, id)?.into_iter().collect();
    let live: Vec<WorktreeState> = WorktreeState::ALL
        .into_iter()
        .filter(|s| *s != WorktreeState::Removed)
        .collect();
    let rows = tree_rows::in_states(conn, &live)?;
    Ok(rows
        .iter()
        .filter(|r| r.task_id.as_ref().is_some_and(|t| tasks.contains(t)))
        .all(|r| r.state == WorktreeState::Rescued))
}

/// Move the idea `id` to `to` through the 1 door; `true` when it moved.
/// A refused move is recorded by the door and gives `false`.
fn moved(ctx: &JobCtx<'_>, tx: &Connection, id: &str, to: &str, text: &str) -> Result<bool> {
    let cause = Cause {
        actor: Actor::Horch,
        text,
    };
    match transition(tx, ctx.guard, Entity::Idea, id, to, cause, ctx.needs_you) {
        Ok(Moved::Changed) => Ok(true),
        Ok(Moved::Unchanged) | Err(DoorError::Refused { .. }) => Ok(false),
        Err(e) => Err(e.into()),
    }
}
