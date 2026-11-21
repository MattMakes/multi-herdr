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

pub const PARKED: &str = "parked";
const CANCELLED: &str = "cancelled";
const MERGED: &str = "merged";

const CANCEL_REQUESTED: &str = "idea cancel requested";

const PASS_REASON: &str = "its idea is cancelled";

const TO_RESCUE: [WorktreeState; 2] = [WorktreeState::Created, WorktreeState::Orphaned];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdeaCancel {
    pub cancelled: bool,
    pub stop: Vec<String>,
}

fn idea(conn: &Connection, id: &str) -> Result<IdeaRow> {
    ideas_store::idea(conn, id)?.with_context(|| format!("no idea {id} in the ledger"))
}

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

fn live_ideas(conn: &Connection, feature: Option<&str>) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM ideas WHERE state NOT IN (?1, ?2)
            AND (?3 IS NULL OR feature_id = ?3)
         ORDER BY id",
    )?;
    let rows = stmt.query_map(params![MERGED, CANCELLED, feature], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

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
