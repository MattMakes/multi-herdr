//! The rows of the table `ingested` (ticket 13).
//!
//! 1 row for each file path that fleetd ingested: a message by its path and
//! its sha256, with the outcome `accepted` or `rejected`. A path with a row
//! is not read again (`docs/specs/fleetd/design.md`, protocol rules 2 and
//! 7).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

/// The outcome of a file that passed each check.
pub const ACCEPTED: &str = "accepted";

/// The outcome of a file that failed a check.
pub const REJECTED: &str = "rejected";

/// Whether `path` has a row.
pub fn has(conn: &Connection, path: &str) -> Result<bool> {
    let row = conn
        .query_row("SELECT 1 FROM ingested WHERE path = ?1", [path], |_| Ok(()))
        .optional()?;
    Ok(row.is_some())
}

/// Write the row of the message `path` with `outcome`, at `at`. False when
/// `path` has a row already: the row stays as it is.
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
