use anyhow::Result;
use rusqlite::{params, Connection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitRow {
    pub kind: String,
    pub reason: String,
    pub started_at: String,
    pub deadline: String,
    pub task_ids: String,
    pub due_at: Option<i64>,
    pub question_id: Option<String>,
}

pub fn of_worker(conn: &Connection, worker: &str) -> Result<Vec<WaitRow>> {
    let mut stmt = conn.prepare(
        "SELECT kind, reason, started_at, deadline, task_ids, due_at, question_id
         FROM worker_waits WHERE worker_id = ?1 ORDER BY kind",
    )?;
    let rows = stmt.query_map([worker], |r| {
        Ok(WaitRow {
            kind: r.get(0)?,
            reason: r.get(1)?,
            started_at: r.get(2)?,
            deadline: r.get(3)?,
            task_ids: r.get(4)?,
            due_at: r.get(5)?,
            question_id: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn put(conn: &Connection, worker: &str, row: &WaitRow) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO worker_waits (worker_id, kind, reason, started_at, deadline,
             task_ids, due_at, question_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            worker,
            row.kind,
            row.reason,
            row.started_at,
            row.deadline,
            row.task_ids,
            row.due_at,
            row.question_id
        ],
    )?;
    Ok(())
}

pub fn set_due_at(conn: &Connection, worker: &str, kind: &str, due_at: i64) -> Result<()> {
    conn.execute(
        "UPDATE worker_waits SET due_at = ?3 WHERE worker_id = ?1 AND kind = ?2",
        params![worker, kind, due_at],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, worker: &str, kind: &str) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM worker_waits WHERE worker_id = ?1 AND kind = ?2",
        params![worker, kind],
    )? > 0)
}
