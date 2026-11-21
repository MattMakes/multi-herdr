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

pub const CURSOR: &str = "verdict_files";

const MODE: u32 = 0o600;

pub fn file_name(n: u32) -> String {
    format!("verdict-{n}.json")
}

pub fn path(fleet_root: &Path, task_id: &str, n: u32) -> PathBuf {
    fleet_root.join("tasks").join(task_id).join(file_name(n))
}

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

pub fn recorded(conn: &Connection, task_id: &str) -> Result<Vec<Verdict>> {
    let rows = door::facts_of(conn, Entity::Task, task_id, EventKind::Verdict)?;
    let mut out = Vec::new();
    for row in &rows {
        out.extend(from_row(row)?);
    }
    Ok(out)
}

pub fn newest(conn: &Connection, task_id: &str) -> Result<Option<Verdict>> {
    Ok(recorded(conn, task_id)?.pop())
}

pub fn newest_of<'a>(
    conn: &Connection,
    task_ids: impl IntoIterator<Item = &'a str>,
) -> Result<std::collections::BTreeMap<String, Verdict>> {
    let mut out = std::collections::BTreeMap::new();
    for row in &door::facts_of_ids(conn, Entity::Task, task_ids, EventKind::Verdict)? {
        if let Some(v) = from_row(row)? {
            out.insert(row.entity_id.clone(), v);
        }
    }
    Ok(out)
}

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
