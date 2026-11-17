//! The `tasks`, `task_deps`, `workers` and `attempts` tables.
//!
//! The SQL of the rows of the reap path. `transition()` writes the state of
//! a row.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, Row};

use crate::fleetd::task_execution::domain::task::{
    AttemptRow, AttemptState, Recorded, Rows, TaskRow, WorkerRow,
};
use crate::taskledger::model::{TaskStatus, WorkerKind, WorkerStatus};
use crate::taskledger::{runs, tasks};

/// A recorded process from its pid and start time columns. A pid that
/// cannot name 1 process is no process.
fn recorded(pid: Option<i64>, start: Option<i64>) -> Option<Recorded> {
    let pid = u32::try_from(pid?).ok().filter(|p| *p > 0)?;
    Some(Recorded {
        pid,
        pid_start: start.and_then(|s| u64::try_from(s).ok()),
    })
}

/// The columns of an attempt. An attempt with no process or pane of its own
/// has those of its worker: the agent can record its pid after its claim.
const ATTEMPT_COLUMNS: &str = "a.id, a.task_id, a.worker_id, a.state,
    COALESCE(a.pid, w.agent_pid), CASE WHEN a.pid IS NULL THEN w.agent_started ELSE a.pid_start END,
    COALESCE(a.mux_handle, w.pane_id)";

fn attempt(r: &Row<'_>) -> rusqlite::Result<AttemptRow> {
    let word: String = r.get(3)?;
    let state = AttemptState::parse(&word).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            anyhow!("attempts.state {word:?}").into(),
        )
    })?;
    Ok(AttemptRow {
        id: r.get(0)?,
        task_id: r.get(1)?,
        worker_id: r.get(2)?,
        state,
        process: recorded(r.get(4)?, r.get(5)?),
        pane: r.get(6)?,
    })
}

fn attempts_where(conn: &Connection, filter: &str, args: &[&str]) -> Result<Vec<AttemptRow>> {
    let sql = format!(
        "SELECT {ATTEMPT_COLUMNS} FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
         {filter} ORDER BY a.rowid"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), attempt)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The attempts of worker `worker_id`, oldest first.
pub fn attempts_of_worker(conn: &Connection, worker_id: &str) -> Result<Vec<AttemptRow>> {
    attempts_where(conn, "WHERE a.worker_id = ?1", &[worker_id])
}

/// The worker row of `r`; `host` is the host of this machine.
fn worker(r: &Row<'_>, host: &str) -> rusqlite::Result<WorkerRow> {
    let status: String = r.get(2)?;
    let kind: String = r.get(1)?;
    let row_host: Option<String> = r.get(3)?;
    let live = WorkerStatus::parse(&status).is_ok_and(WorkerStatus::is_live);
    Ok(WorkerRow {
        id: r.get(0)?,
        live,
        orchestrator: kind == WorkerKind::Orchestrator.as_str(),
        here: row_host.is_none_or(|h| h == host),
        agent: recorded(r.get(4)?, r.get(5)?),
        supervisor: recorded(r.get(6)?, r.get(7)?),
        pane: r.get(8)?,
        status,
    })
}

fn workers(conn: &Connection, host: &str) -> Result<BTreeMap<String, WorkerRow>> {
    let mut stmt = conn.prepare(
        "SELECT worker_id, kind, status, host, agent_pid, agent_started, supervisor_pid,
            supervisor_started, pane_id
         FROM workers",
    )?;
    let rows = stmt.query_map([], |r| worker(r, host))?;
    let mut out = BTreeMap::new();
    for w in rows {
        let w = w?;
        out.insert(w.id.clone(), w);
    }
    Ok(out)
}

/// The task row of task `task_id`.
fn task(conn: &Connection, task_id: &str) -> Result<TaskRow> {
    let t = tasks::get(conn, task_id)?;
    let max_attempts = runs::get(conn, &t.run_id)?.max_attempts;
    let holds = [TaskStatus::Running, TaskStatus::Landing]
        .iter()
        .any(|s| s.as_str() == t.status);
    Ok(TaskRow {
        held_by: t.current_worker_id.filter(|_| holds),
        used: t.attempts - t.attempt_floor,
        max_attempts,
        id: t.task_id,
    })
}

/// The rows of the reap path: each attempt and each worker row, and the
/// task of each live attempt. `host` is the host of this machine.
pub fn rows(conn: &Connection, host: &str) -> Result<Rows> {
    let attempts = attempts_where(conn, "", &[])?;
    let mut tasks = BTreeMap::new();
    let live = attempts
        .iter()
        .filter(|a| a.state == AttemptState::Live)
        .filter_map(|a| a.task_id.as_deref());
    for t in live {
        if !tasks.contains_key(t) {
            tasks.insert(t.to_string(), task(conn, t)?);
        }
    }
    Ok(Rows {
        attempts,
        workers: workers(conn, host)?,
        tasks,
    })
}

/// Insert `a`, started at `now`. Its event is the caller's.
pub fn insert_attempt(conn: &Connection, a: &AttemptRow, now: &str) -> Result<()> {
    let p = a.process;
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, mux_handle, pid, pid_start, state, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            a.id,
            a.task_id,
            a.worker_id,
            a.pane,
            p.map(|p| i64::from(p.pid)),
            p.and_then(|p| p.pid_start).and_then(|s| i64::try_from(s).ok()),
            a.state.as_str(),
            now
        ],
    )?;
    Ok(())
}

/// Write the outcome and the end time of the attempt `id`, which
/// `transition()` moves to `ended`.
pub fn set_ended(conn: &Connection, id: &str, outcome: &str, now: &str) -> Result<()> {
    conn.execute(
        "UPDATE attempts SET outcome = ?2, ended_at = COALESCE(ended_at, ?3) WHERE id = ?1",
        params![id, outcome, now],
    )?;
    Ok(())
}
