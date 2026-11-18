//! Writes `verdict.json` of an attempt (ticket 19).
//!
//! Each verdict of the verifier is a row first, then a file after the
//! commit (`docs/specs/fleetd/design.md`, A24; protocol rules 1 to 3):
//!
//! - The row is a `verdict` fact of the task, recorded through the door,
//!   with the cause `verdict-<n>.json` and the verdict in `data_json`. `n`
//!   counts the verdicts of the task: 1, 2, and so on.
//! - The file is `<fleet root>/tasks/<task id>/verdict-<n>.json`, a new
//!   name for each verdict. It is written 1 time through
//!   [`fsx::create_immutable`]: the same bytes again are accepted, and other
//!   bytes are refused.
//! - The cursor [`CURSOR`] in `pass_cursors` holds the event id of the last
//!   row whose file is written. A crash after the commit of a row and
//!   before its file leaves the cursor before the row, so the next call
//!   writes the file.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::cursors_store;
use crate::fleetd::fleet_runtime::adapters::sqlite::{self as door, FactRow};
use crate::fleetd::fleet_runtime::application::ledger::{self, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::task_execution::domain::verification::{Decision, Verdict};
use crate::fsx;
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::util;

/// The `pass_cursors` kind of the verdict files.
pub const CURSOR: &str = "verdict_files";

/// The file mode: only the user of fleetd and its workers.
const MODE: u32 = 0o600;

/// The name of verdict `n`.
pub fn file_name(n: u32) -> String {
    format!("verdict-{n}.json")
}

/// The file of verdict `n` of task `task_id`.
pub fn path(fleet_root: &Path, task_id: &str, n: u32) -> PathBuf {
    fleet_root.join("tasks").join(task_id).join(file_name(n))
}

/// The form of a verdict in its row and its file.
#[derive(Serialize, Deserialize)]
struct Wire {
    task_id: String,
    attempt: String,
    n: u32,
    decision: String,
    tip: String,
    check_hash: String,
    job: Option<i64>,
    reason: String,
}

fn to_wire(v: &Verdict) -> Wire {
    Wire {
        task_id: v.task_id.clone(),
        attempt: v.attempt.clone(),
        n: v.n,
        decision: v.decision.as_str().into(),
        tip: v.tip.clone(),
        check_hash: v.check_hash.clone(),
        job: v.job,
        reason: v.reason.clone(),
    }
}

/// The verdict of a verdict row, or `None` for another `verdict` fact.
fn from_row(row: &FactRow) -> Result<Option<Verdict>> {
    if !(row.cause.starts_with("verdict-") && row.cause.ends_with(".json")) {
        return Ok(None);
    }
    let w: Wire = serde_json::from_str(&row.data_json)
        .with_context(|| format!("the verdict row {}", row.event_id))?;
    let decision = Decision::parse(&w.decision).with_context(|| {
        format!(
            "the verdict row {}: decision {:?}",
            row.event_id, w.decision
        )
    })?;
    Ok(Some(Verdict {
        task_id: w.task_id,
        attempt: w.attempt,
        n: w.n,
        decision,
        tip: w.tip,
        check_hash: w.check_hash,
        job: w.job,
        reason: w.reason,
    }))
}

/// Record `v` as the next verdict of its task, in the open transaction
/// `tx`. The fleetd of generation `guard` writes. Gives the verdict with
/// its number.
pub fn record(tx: &Connection, guard: Option<Generation>, mut v: Verdict) -> Result<Verdict> {
    let count = recorded(tx, &v.task_id)?.len();
    v.n = u32::try_from(count + 1).context("the count of verdicts of a task")?;
    let data = serde_json::to_value(to_wire(&v)).context("a verdict serializes")?;
    let name = file_name(v.n);
    ledger::record(
        tx,
        guard,
        EventKind::Verdict,
        Entity::Task,
        &v.task_id,
        Cause {
            actor: Actor::Horch,
            text: &name,
        },
        &data,
    )?;
    Ok(v)
}

/// Each verdict of task `task_id`, oldest first.
pub fn recorded(conn: &Connection, task_id: &str) -> Result<Vec<Verdict>> {
    let rows = door::facts_of(conn, Entity::Task, task_id, EventKind::Verdict)?;
    let mut out = Vec::new();
    for row in &rows {
        out.extend(from_row(row)?);
    }
    Ok(out)
}

/// The newest verdict of task `task_id`.
pub fn newest(conn: &Connection, task_id: &str) -> Result<Option<Verdict>> {
    Ok(recorded(conn, task_id)?.pop())
}

/// Write the file of each verdict row after [`CURSOR`], oldest first, then
/// move the cursor. Gives the files that this call wrote or found with the
/// same bytes. A file with other bytes stops the call, and the cursor stays
/// before its row.
pub fn write_unwritten(conn: &mut Connection, fleet_root: &Path) -> Result<Vec<PathBuf>> {
    let cursor = cursors_store::load(conn, CURSOR, &util::now())?;
    let after: i64 = cursor
        .after_key
        .as_deref()
        .map(str::parse)
        .transpose()
        .context("the cursor of the verdict files")?
        .unwrap_or(0);
    let rows = door::facts_after(conn, Entity::Task, EventKind::Verdict, after)?;
    let Some(last) = rows.last().map(|r| r.event_id) else {
        return Ok(Vec::new());
    };
    let mut written = Vec::new();
    for row in &rows {
        let Some(v) = from_row(row)? else {
            continue;
        };
        let file = path(fleet_root, &v.task_id, v.n);
        let dir = file.parent().context("a verdict file has a folder")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let bytes = serde_json::to_vec(&to_wire(&v)).context("a verdict serializes")?;
        fsx::create_immutable(&file, &bytes, MODE)
            .with_context(|| format!("writing {}", file.display()))?;
        written.push(file);
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut moved = cursors_store::load(&tx, CURSOR, &util::now())?;
    moved.after_key = Some(last.to_string());
    cursors_store::save(&tx, CURSOR, &moved)?;
    tx.commit()?;
    Ok(written)
}
