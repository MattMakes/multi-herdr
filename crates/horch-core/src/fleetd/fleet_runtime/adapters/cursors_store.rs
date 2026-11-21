use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::fleetd::fleet_runtime::domain::budget::Cursor;

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
