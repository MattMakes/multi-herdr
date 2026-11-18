//! The use case of the disk budget of the worktrees.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" ("Disk"), FM-28, OR-1; MAP.md of fleetd-p3, Q8, Q9 and Q20.
//! A tree is spare when the idea of its task is `merged` or `cancelled`
//! (ticket 33 of fleetd-p6): a row has no idea, so its task names it.
//! [`disk_budget`] runs in part 2 of a pass, after `reconcile_worktrees`.
//! It reads the sizes that the `worktree_list` job stored and runs no git.
//!
//! It queues 1 `worktree_remove` job for each tree of the decision (Q9).
//! The job runs outside the pass, and its guard checks the tree again at
//! its removal.
//!
//! The hold: when the total stays above the budget after each tree of the
//! decision, 1 needs-you item is open with the total and the budget. It is
//! resolved when the decision no longer holds. [`decision`] is also the
//! read of the global hold `WorktreeDisk` (`holds()` of fleet runtime).

use anyhow::Result;
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::rollup::idea_state_of_task;
use crate::fleetd::worktree_custody::adapters::sqlite as store;
use crate::fleetd::worktree_custody::application::remove::{queue_auto_remove, removal_failed};
use crate::fleetd::worktree_custody::domain::disk::{
    self as rules, Decision, Tree, BUDGET_KEY, GB, NEEDS_YOU_KEY, NEEDS_YOU_KIND,
};

/// What 1 call of [`disk_budget`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The sum of the measured sizes of all trees, in bytes.
    pub total: u64,
    /// The budget, in bytes.
    pub budget: u64,
    /// The trees of the decision that got a new `worktree_remove` run,
    /// oldest first.
    pub removals: Vec<String>,
    /// The cause of the hold when the total stays above the budget: new
    /// dispatch holds, and the needs-you item is open.
    pub hold: Option<String>,
}

/// 1 row that is not `removed`, with the state of the idea of its task and
/// whether its automatic removal failed.
struct Measured {
    id: String,
    state: String,
    has_task: bool,
    idea_state: Option<String>,
    created_at: String,
    bytes: Option<u64>,
    removal_failed: bool,
}

/// The decision of the budget of `budget_gb` GB for the trees in `conn`.
pub fn decision(conn: &Connection, budget_gb: u64) -> Result<Decision> {
    let rows = measured(conn)?;
    let trees: Vec<Tree<'_>> = rows
        .iter()
        .map(|r| Tree {
            id: &r.id,
            state: &r.state,
            has_task: r.has_task,
            idea_state: r.idea_state.as_deref(),
            created_at: &r.created_at,
            bytes: r.bytes,
            removal_failed: r.removal_failed,
        })
        .collect();
    Ok(rules::decide(&trees, budget_gb))
}

/// The disk budget in `tx`, at `clock` (part 2 of a pass): queue the
/// removal of each tree of the decision, and open or resolve the needs-you
/// item of the hold.
pub fn disk_budget(ctx: &JobCtx<'_>, tx: &Connection, clock: FleetSeconds) -> Result<Report> {
    let d = decision(tx, ctx.config.worktree_disk_budget_gb)?;
    let removals = queue_removals(ctx, tx, &d, clock)?;
    sync_hold_item(ctx, tx, &d)?;
    Ok(Report {
        total: d.total,
        budget: d.budget,
        removals,
        hold: d.hold.then(|| rules::hold_cause(&d)),
    })
}

/// Queue 1 `worktree_remove` run of each tree of `d`, oldest first. A
/// tree with an active run, or whose same removal failed, is not queued
/// again. Give the trees with a new run.
fn queue_removals(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    d: &Decision,
    clock: FleetSeconds,
) -> Result<Vec<String>> {
    let mut removals = Vec::new();
    for id in &d.remove {
        let Some(row) = store::get(tx, id)? else {
            continue;
        };
        let cause = "the worktrees are above their disk budget";
        if queue_auto_remove(ctx, tx, &row, cause, clock)? {
            removals.push(id.clone());
        }
    }
    Ok(removals)
}

/// Open the needs-you item of the hold of `d` with the total and the
/// budget, or resolve it when `d` does not hold.
fn sync_hold_item(ctx: &JobCtx<'_>, tx: &Connection, d: &Decision) -> Result<()> {
    if !d.hold {
        ctx.needs_you.resolve_item(tx, NEEDS_YOU_KEY)?;
        return Ok(());
    }
    let text = format!(
        "The worktrees use {:.1} GB, above the budget of {} GB ({BUDGET_KEY}). fleetd holds \
         new dispatch: no landed or rescued tree of a merged or cancelled idea is left to \
         remove, and fleetd never removes an active tree for space. Land, drop or clean up \
         trees, or raise {BUDGET_KEY}.",
        d.total as f64 / GB as f64,
        d.budget / GB
    );
    let item = NewItem {
        key: NEEDS_YOU_KEY,
        kind: NEEDS_YOU_KIND,
        entity: Entity::Worktree,
        entity_id: BUDGET_KEY,
        text: &text,
    };
    ctx.needs_you.open_item(tx, &item)
}

/// Each row that is not `removed`, with the state of the idea of its task.
fn measured(conn: &Connection) -> Result<Vec<Measured>> {
    let mut stmt = conn.prepare(
        "SELECT worktree_id, state, task_id, created_at, bytes_on_disk, path
           FROM worktrees WHERE state <> 'removed'",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, state, task_id, created_at, bytes, path) in rows {
        let idea_state = match task_id.as_deref() {
            Some(task) => idea_state_of_task(conn, task)?,
            None => None,
        };
        let removal_failed = removal_failed(conn, &id, &path)?;
        out.push(Measured {
            id,
            state,
            has_task: task_id.is_some(),
            idea_state,
            created_at,
            bytes: bytes.and_then(|b| u64::try_from(b).ok()),
            removal_failed,
        });
    }
    Ok(out)
}
