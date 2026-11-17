//! The `pass_cursors` table.
//!
//! Part 1 of a pass has no transaction: it [`load`]s each cursor, and the
//! scans move the cursors in memory. Part 2 [`save`]s them in its
//! transaction, so a crash before the commit scans the same items again,
//! and no item is skipped.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::fleetd::fleet_runtime::domain::budget::Cursor;

/// The cursor of `kind`. No row gives a new cursor made at `now`.
pub fn load(conn: &Connection, kind: &str, now: &str) -> Result<Cursor> {
    let row = conn
        .query_row(
            "SELECT after_key, lap_started_at, prev_lap_started_at FROM pass_cursors WHERE kind = ?1",
            [kind],
            |r| {
                Ok(Cursor {
                    after_key: r.get(0)?,
                    lap_started_at: r.get(1)?,
                    prev_lap_started_at: r.get(2)?,
                })
            },
        )
        .optional()
        .with_context(|| format!("reading the cursor {kind}"))?;
    Ok(row.unwrap_or_else(|| Cursor::new(now)))
}

/// Write the cursor of `kind` in the transaction of part 2 of the pass.
pub fn save(tx: &Transaction<'_>, kind: &str, cursor: &Cursor) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO pass_cursors (kind, after_key, lap_started_at, prev_lap_started_at)
         VALUES (?1, ?2, ?3, ?4)",
        (
            kind,
            &cursor.after_key,
            &cursor.lap_started_at,
            &cursor.prev_lap_started_at,
        ),
    )
    .with_context(|| format!("writing the cursor {kind}"))?;
    Ok(())
}
