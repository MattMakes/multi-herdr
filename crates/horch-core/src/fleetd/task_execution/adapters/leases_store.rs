//! The SQLite store of the leases: the `leases` table and
//! `attempts.active_at`. Each time in them is on the fleet clock, except
//! `renewed_at`, which is calendar time for the reader.
//!
//! The primary key of `leases` is `task_id`, so a task has at most 1 lease
//! row (FM-14).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::fleetd::task_execution::domain::lease::Holder;
use crate::fleetd::task_execution::domain::task::AttemptState;
use crate::taskledger::model::WorkerStatus;

/// 1 row of `leases`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseRow {
    pub task_id: String,
    pub attempt_id: String,
    pub expires_at: i64,
    pub renew_basis: String,
}

/// 1 lease with the state of its task and of its attempt: what a pass
/// judges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldLease {
    pub task_id: String,
    pub task_status: String,
    pub attempt_id: String,
    /// The attempt is not ended.
    pub attempt_live: bool,
    pub worker_id: String,
    pub expires_at: i64,
    /// The fleet clock of the last activity of the attempt.
    pub active_at: Option<i64>,
}

/// 1 attempt that is not ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveAttempt {
    pub id: String,
    pub task_id: String,
    pub worker_id: String,
    /// The worker row has a live status. A handoff or a reap ends the
    /// worker row before the pass ends the attempt.
    pub worker_live: bool,
}

/// The `IN (...)` list of the task words that hold a lease.
fn held_words() -> String {
    Holder::ALL
        .iter()
        .map(|h| format!("'{}'", h.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The lease of task `task_id`.
pub fn get(conn: &Connection, task_id: &str) -> Result<Option<LeaseRow>> {
    Ok(conn
        .query_row(
            "SELECT task_id, attempt_id, expires_at, renew_basis FROM leases WHERE task_id = ?1",
            [task_id],
            |r| {
                Ok(LeaseRow {
                    task_id: r.get(0)?,
                    attempt_id: r.get(1)?,
                    expires_at: r.get(2)?,
                    renew_basis: r.get(3)?,
                })
            },
        )
        .optional()?)
}

/// Write the lease of `l.task_id`, renewed at `now`. It replaces the row
/// of the task, when it has one.
pub fn put(conn: &Connection, l: &LeaseRow, now: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![l.task_id, l.attempt_id, l.expires_at, now, l.renew_basis],
    )?;
    Ok(())
}

/// Move the deadline of the lease of `task_id` to `expires_at`, renewed at
/// `now` on `basis`.
pub fn renew(
    conn: &Connection,
    task_id: &str,
    expires_at: i64,
    basis: &str,
    now: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE leases SET expires_at = ?2, renewed_at = ?3, renew_basis = ?4 WHERE task_id = ?1",
        params![task_id, expires_at, now, basis],
    )?;
    Ok(())
}

/// Delete the lease of `task_id`. `false` when it had none.
pub fn delete(conn: &Connection, task_id: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM leases WHERE task_id = ?1", [task_id])? > 0)
}

/// Set the last activity of attempt `attempt_id` to fleet clock `clock`.
pub fn set_active_at(conn: &Connection, attempt_id: &str, clock: i64) -> Result<()> {
    conn.execute(
        "UPDATE attempts SET active_at = ?2 WHERE id = ?1",
        params![attempt_id, clock],
    )?;
    Ok(())
}

/// Whether attempt `attempt_id` exists and is not ended.
pub fn attempt_live(conn: &Connection, attempt_id: &str) -> Result<bool> {
    let state: Option<String> = conn
        .query_row(
            "SELECT state FROM attempts WHERE id = ?1",
            [attempt_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(state.is_some_and(|s| s != AttemptState::Ended.as_str()))
}

/// Each lease, with its task and its attempt, in task order.
pub fn held(conn: &Connection) -> Result<Vec<HeldLease>> {
    let mut stmt = conn.prepare(
        "SELECT l.task_id, t.status, l.attempt_id, a.state, a.worker_id, l.expires_at, a.active_at
         FROM leases l JOIN tasks t ON t.task_id = l.task_id
         JOIN attempts a ON a.id = l.attempt_id
         ORDER BY l.task_id",
    )?;
    let rows = stmt.query_map([], |r| {
        let state: String = r.get(3)?;
        Ok(HeldLease {
            task_id: r.get(0)?,
            task_status: r.get(1)?,
            attempt_id: r.get(2)?,
            attempt_live: state != AttemptState::Ended.as_str(),
            worker_id: r.get(4)?,
            expires_at: r.get(5)?,
            active_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 1 task that holds a lease or has a lease row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeasedTask {
    pub task_id: String,
    /// The `tasks.status` word.
    pub status: String,
    /// The attempt of its lease row, when it has one.
    pub lease_attempt: Option<String>,
}

/// The lease facts of I1: each task that holds a lease or has a lease row,
/// in task order, and the attempts of those tasks that are not ended,
/// oldest first.
pub fn i1_rows(conn: &Connection) -> Result<(Vec<LeasedTask>, Vec<LiveAttempt>)> {
    let tasks = format!(
        "SELECT t.task_id, t.status, l.attempt_id FROM tasks t
         LEFT JOIN leases l ON l.task_id = t.task_id
         WHERE t.status IN ({held}) OR l.task_id IS NOT NULL
         ORDER BY t.task_id",
        held = held_words()
    );
    let tasks = conn
        .prepare(&tasks)?
        .query_map([], |r| {
            Ok(LeasedTask {
                task_id: r.get(0)?,
                status: r.get(1)?,
                lease_attempt: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let live = format!(
        "SELECT a.id, a.task_id, a.worker_id, w.status FROM attempts a
         JOIN tasks t ON t.task_id = a.task_id JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.state <> '{ended}' AND (t.status IN ({held})
            OR t.task_id IN (SELECT task_id FROM leases))
         ORDER BY a.rowid",
        ended = AttemptState::Ended.as_str(),
        held = held_words()
    );
    let live = conn
        .prepare(&live)?
        .query_map([], |r| {
            let status: String = r.get(3)?;
            Ok(LiveAttempt {
                id: r.get(0)?,
                task_id: r.get(1)?,
                worker_id: r.get(2)?,
                worker_live: WorkerStatus::parse(&status).is_ok_and(WorkerStatus::is_live),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok((tasks, live))
}
