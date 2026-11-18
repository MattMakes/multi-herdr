//! The use case of the disk budget of the worktrees, in report-only mode.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" ("Disk"), FM-28, OR-1; MAP.md of fleetd-p3, Q8, Q9 and Q20.
//! [`disk_budget`] runs in part 2 of a pass, after `reconcile_worktrees`.
//! It reads the sizes that the `worktree_list` job stored and runs no git.
//!
//! Report-only mode (Q9): it queues no remove job. It reports each tree of
//! the decision that has no active `worktree_remove` run, with 1 event for
//! each row. Ticket 18 of fleetd-p3 queues the removals.
//!
//! The hold: when the total stays above the budget after each tree of the
//! decision, 1 needs-you item is open with the total and the budget. It is
//! resolved when the decision no longer holds. [`decision`] is also the
//! read of the global hold `WorktreeDisk` (`holds()` of fleet runtime).

use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::adapters::sqlite::has_fact;
use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::application::ledger::{record, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::Kind;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::worktree_custody::domain::disk::{
    self as rules, Decision, Tree, BUDGET_KEY, GB, NEEDS_YOU_KEY, NEEDS_YOU_KIND, WOULD_REMOVE,
};
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::{runs, tasks};

/// What 1 call of [`disk_budget`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The sum of the measured sizes of all trees, in bytes.
    pub total: u64,
    /// The budget, in bytes.
    pub budget: u64,
    /// The trees of the decision with no active `worktree_remove` run,
    /// oldest first: what report-only mode does not queue.
    pub would_remove: Vec<String>,
    /// The cause of the hold when the total stays above the budget: new
    /// dispatch holds, and the needs-you item is open.
    pub hold: Option<String>,
}

/// 1 row that is not `removed`, with the status of its run.
struct Measured {
    id: String,
    state: String,
    has_task: bool,
    run_status: Option<String>,
    created_at: String,
    bytes: Option<u64>,
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
            run_status: r.run_status.as_deref(),
            created_at: &r.created_at,
            bytes: r.bytes,
        })
        .collect();
    Ok(rules::decide(&trees, budget_gb))
}

/// The disk budget in `tx`, at `clock` (part 2 of a pass): report the
/// trees that a removal for space would take, and open or resolve the
/// needs-you item of the hold.
pub fn disk_budget(ctx: &JobCtx<'_>, tx: &Connection, clock: FleetSeconds) -> Result<Report> {
    let d = decision(tx, ctx.config.worktree_disk_budget_gb)?;
    let would_remove = report_removals(ctx, tx, &d, clock)?;
    sync_hold_item(ctx, tx, &d)?;
    Ok(Report {
        total: d.total,
        budget: d.budget,
        would_remove,
        hold: d.hold.then(|| rules::hold_cause(&d)),
    })
}

/// The trees of `d` with no active `worktree_remove` run, each with 1
/// event the first time it is reported (report-only mode, Q9).
fn report_removals(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    d: &Decision,
    clock: FleetSeconds,
) -> Result<Vec<String>> {
    let mut would_remove = Vec::new();
    for id in &d.remove {
        if jobs_store::active(tx, Kind::WorktreeRemove, id)?.is_some() {
            continue;
        }
        would_remove.push(id.clone());
        if has_fact(tx, Entity::Worktree, id, EventKind::Note, WOULD_REMOVE)? {
            continue;
        }
        let data = json!({
            "total_bytes": d.total,
            "budget_bytes": d.budget,
            "fleet_clock": clock.0,
        });
        let cause = Cause {
            actor: Actor::Horch,
            text: WOULD_REMOVE,
        };
        record(
            tx,
            ctx.guard,
            EventKind::Note,
            Entity::Worktree,
            id,
            cause,
            &data,
        )?;
    }
    Ok(would_remove)
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
         new dispatch: no landed or rescued tree of a done or abandoned run is left to \
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

/// Each row that is not `removed`, with the status of the run of its task
/// or of its mode row.
fn measured(conn: &Connection) -> Result<Vec<Measured>> {
    let mut stmt = conn.prepare(
        "SELECT w.worktree_id, w.state, w.task_id, w.created_at, w.bytes_on_disk, m.run_id
           FROM worktrees w LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
          WHERE w.state <> 'removed'",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, state, task_id, created_at, bytes, mode_run) in rows {
        let run_id = match task_id.as_deref() {
            Some(task) => Some(tasks::get(conn, task)?.run_id),
            None => mode_run,
        };
        let run_status = match run_id {
            Some(run) => Some(runs::get(conn, &run)?.status),
            None => None,
        };
        out.push(Measured {
            id,
            state,
            has_task: task_id.is_some(),
            run_status,
            created_at,
            bytes: bytes.and_then(|b| u64::try_from(b).ok()),
        });
    }
    Ok(out)
}
