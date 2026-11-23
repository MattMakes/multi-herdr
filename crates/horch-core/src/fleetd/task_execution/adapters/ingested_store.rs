use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

pub const ACCEPTED: &str = "accepted";

pub const REJECTED: &str = "rejected";

pub fn has(conn: &Connection, path: &str) -> Result<bool> {
    let row = conn
        .query_row("SELECT 1 FROM ingested WHERE path = ?1", [path], |_| Ok(()))
        .optional()?;
    Ok(row.is_some())
}

pub fn record_message(
    conn: &Connection,
    path: &str,
    sha256: Option<&str>,
    outcome: &str,
    reason: Option<&str>,
    at: &str,
) -> Result<bool> {
    let n = conn.execute(
        "INSERT INTO ingested (path, sha256, outcome, reason, at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (path) DO NOTHING",
        params![path, sha256, outcome, reason, at],
    )?;
    Ok(n == 1)
}
