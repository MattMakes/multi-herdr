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

/// The worker and the `via` (`claim` or `push`) of each attempt of task
/// `task_id`, oldest first.
pub fn attempt_vias(conn: &Connection, task_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt =
        conn.prepare("SELECT worker_id, via FROM attempts WHERE task_id = ?1 ORDER BY rowid")?;
    let rows = stmt.query_map([task_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Each attempt that is not ended and that has a pid, with the persona of
/// its worker, oldest first: the facts of the peer process check of the
/// command channel (SR-8).
pub fn open_attempts_with_persona(conn: &Connection) -> Result<Vec<(AttemptRow, Option<String>)>> {
    let sql = format!(
        "SELECT {ATTEMPT_COLUMNS}, w.persona FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.state != 'ended' AND COALESCE(a.pid, w.agent_pid) IS NOT NULL ORDER BY a.rowid"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| Ok((attempt(r)?, r.get(7)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
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

/// Each task of the idea `idea_id`, as (`task_id`, `status`), oldest first.
pub fn task_states_of_idea(conn: &Connection, idea_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt =
        conn.prepare("SELECT task_id, status FROM tasks WHERE idea_id = ?1 ORDER BY rowid")?;
    let rows = stmt.query_map([idea_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The task row of task `task_id`.
fn task(conn: &Connection, task_id: &str) -> Result<TaskRow> {
    let t = tasks::get(conn, task_id)?;
    let max_attempts = runs::get(conn, &t.run_id)?.max_attempts;
    let holds = [TaskStatus::Running, TaskStatus::Verifying]
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

/// Record the process `p` of the attempt `id`: the launch of fleetd started
/// it. `transition()` writes the state.
pub fn set_process(conn: &Connection, id: &str, p: Recorded) -> Result<()> {
    conn.execute(
        "UPDATE attempts SET pid = ?2, pid_start = ?3 WHERE id = ?1",
        params![
            id,
            i64::from(p.pid),
            p.pid_start.and_then(|s| i64::try_from(s).ok())
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

/// The tasks of the idea `idea_id` that are not `done` and not `cancelled`,
/// by id.
pub fn open_tasks_of_idea(conn: &Connection, idea_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT task_id FROM tasks WHERE idea_id = ?1 AND status NOT IN (?2, ?3)
         ORDER BY task_id",
    )?;
    let ended = [TaskStatus::Done.as_str(), TaskStatus::Cancelled.as_str()];
    let ids = stmt
        .query_map(params![idea_id, ended[0], ended[1]], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

/// Each task of the idea `idea_id`, by id.
pub fn tasks_of_idea(conn: &Connection, idea_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT task_id FROM tasks WHERE idea_id = ?1 ORDER BY task_id")?;
    let ids = stmt
        .query_map([idea_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

/// The idea of the task `task_id`, or `None`.
pub fn idea_of_task(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let idea = conn.query_row(
        "SELECT idea_id FROM tasks WHERE task_id = ?1",
        [task_id],
        |r| r.get(0),
    )?;
    Ok(idea)
}

/// A live attempt with its own recorded wrapper process, and its task or,
/// for a sub-orchestrator attempt, its idea (fleetd-p6 ticket 25).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveWrapper {
    pub id: String,
    pub task_id: Option<String>,
    pub idea_id: Option<String>,
    pub pid: u32,
    pub pid_start: u64,
}

/// Each live attempt whose wrapper PID and start time are recorded. It
/// only reads.
pub fn live_wrappers(conn: &Connection) -> Result<Vec<LiveWrapper>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, idea_id, pid, pid_start FROM attempts
         WHERE state = 'live' AND pid IS NOT NULL AND pid_start IS NOT NULL
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            recorded(r.get(3)?, r.get(4)?),
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, task_id, idea_id, process) = row?;
        // A value that is not a PID or a start time names no process.
        if let Some(Recorded {
            pid,
            pid_start: Some(pid_start),
        }) = process
        {
            out.push(LiveWrapper {
                id,
                task_id,
                idea_id,
                pid,
                pid_start,
            });
        }
    }
    Ok(out)
}

/// The task and the idea of attempt `id`. `None` when it has no row.
pub fn task_and_idea(
    conn: &Connection,
    id: &str,
) -> Result<Option<(Option<String>, Option<String>)>> {
    use rusqlite::OptionalExtension as _;
    Ok(conn
        .query_row(
            "SELECT task_id, idea_id FROM attempts WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}
