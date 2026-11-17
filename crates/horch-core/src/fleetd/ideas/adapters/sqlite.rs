//! The `needs_you` table.
//!
//! The SQL of the rows. `transition()` writes the state of a row.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::fleetd::ideas::domain::needs_you::{Item, State};

const COLUMNS: &str = "key, kind, entity, entity_id, state, text, created_at, ended_at";

fn item(r: &Row<'_>) -> rusqlite::Result<Item> {
    let word: String = r.get(4)?;
    let state = State::parse(&word).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            anyhow!("needs_you.state {word:?}").into(),
        )
    })?;
    Ok(Item {
        key: r.get(0)?,
        kind: r.get(1)?,
        entity: r.get(2)?,
        entity_id: r.get(3)?,
        state,
        text: r.get(5)?,
        created_at: r.get(6)?,
        ended_at: r.get(7)?,
    })
}

/// The item of `key`.
pub fn get(conn: &Connection, key: &str) -> Result<Option<Item>> {
    let sql = format!("SELECT {COLUMNS} FROM needs_you WHERE key = ?1");
    Ok(conn.query_row(&sql, [key], item).optional()?)
}

/// Insert `i` as the new row of its key.
pub fn insert(conn: &Connection, i: &Item) -> Result<()> {
    let sql = format!("INSERT INTO needs_you ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)");
    conn.execute(
        &sql,
        params![
            i.key,
            i.kind,
            i.entity,
            i.entity_id,
            i.state.as_str(),
            i.text,
            i.created_at,
            i.ended_at
        ],
    )?;
    Ok(())
}

/// Write each column of `i` but its state, which only `transition()`
/// writes.
pub fn update(conn: &Connection, i: &Item) -> Result<()> {
    conn.execute(
        "UPDATE needs_you SET kind = ?2, entity = ?3, entity_id = ?4, text = ?5,
            created_at = ?6, ended_at = ?7
         WHERE key = ?1",
        params![
            i.key,
            i.kind,
            i.entity,
            i.entity_id,
            i.text,
            i.created_at,
            i.ended_at
        ],
    )?;
    Ok(())
}

/// The open items, oldest first.
pub fn open(conn: &Connection) -> Result<Vec<Item>> {
    let sql = format!("SELECT {COLUMNS} FROM needs_you WHERE state = ?1 ORDER BY created_at, key");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([State::Open.as_str()], item)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Remove each item that is not open and ended at or before `cutoff`.
/// Ledger stamps have 1 format, so they compare as text.
pub fn delete_ended(conn: &Connection, cutoff: &str) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM needs_you WHERE state <> ?1 AND ended_at <= ?2",
        params![State::Open.as_str(), cutoff],
    )?)
}
