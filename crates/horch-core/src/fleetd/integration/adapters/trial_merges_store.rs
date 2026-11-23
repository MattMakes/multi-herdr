use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::fleetd::fleet_runtime::adapters::sqlite::check_writable;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::integration::domain::trial_merge::{Pair, Verdict};
use crate::taskledger::util;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialMergeRow {
    pub left_ref: String,
    pub right_ref: String,
    pub left_tip: String,
    pub right_tip: String,
    pub job_id: i64,
    pub clean: Option<bool>,
    pub conflict_files: Vec<String>,
    pub at: String,
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<TrialMergeRow> {
    let files: Option<String> = r.get("conflict_files")?;
    let conflict_files = match files {
        Some(text) => serde_json::from_str(&text).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
        })?,
        None => Vec::new(),
    };
    Ok(TrialMergeRow {
        left_ref: r.get("left_ref")?,
        right_ref: r.get("right_ref")?,
        left_tip: r.get("left_tip")?,
        right_tip: r.get("right_tip")?,
        job_id: r.get("job_id")?,
        clean: r.get("clean")?,
        conflict_files,
        at: r.get("at")?,
    })
}

pub fn put_queued(
    tx: &Connection,
    guard: Option<Generation>,
    pair: &Pair,
    job_id: i64,
) -> Result<()> {
    check_writable(tx, guard)?;
    tx.execute(
        "INSERT OR REPLACE INTO trial_merges
             (left_ref, right_ref, left_tip, right_tip, job_id, clean, conflict_files, at)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6)",
        params![
            pair.left.ref_name,
            pair.right.ref_name,
            pair.left.tip,
            pair.right.tip,
            job_id,
            util::now()
        ],
    )
    .with_context(|| format!("writing the trial merge {}", pair.subject()))?;
    Ok(())
}

pub fn set_verdict(
    tx: &Connection,
    guard: Option<Generation>,
    left_tip: &str,
    right_tip: &str,
    job_id: i64,
    verdict: &Verdict,
) -> Result<()> {
    check_writable(tx, guard)?;
    let (clean, files) = match verdict {
        Verdict::Clean => (true, None),
        Verdict::Conflict(files) => (false, Some(serde_json::to_string(files)?)),
    };
    tx.execute(
        "UPDATE trial_merges SET job_id = ?3, clean = ?4, conflict_files = ?5, at = ?6
         WHERE left_tip = ?1 AND right_tip = ?2",
        params![left_tip, right_tip, job_id, clean, files, util::now()],
    )?;
    Ok(())
}

pub fn set_job(
    tx: &Connection,
    guard: Option<Generation>,
    left_tip: &str,
    right_tip: &str,
    job_id: i64,
) -> Result<()> {
    check_writable(tx, guard)?;
    tx.execute(
        "UPDATE trial_merges SET job_id = ?3 WHERE left_tip = ?1 AND right_tip = ?2",
        params![left_tip, right_tip, job_id],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, left_tip: &str, right_tip: &str) -> Result<Option<TrialMergeRow>> {
    conn.query_row(
        "SELECT * FROM trial_merges WHERE left_tip = ?1 AND right_tip = ?2",
        [left_tip, right_tip],
        from_row,
    )
    .optional()
    .context("reading a trial_merges row")
}

pub fn waiting(conn: &Connection) -> Result<Vec<TrialMergeRow>> {
    let mut stmt = conn.prepare("SELECT * FROM trial_merges WHERE clean IS NULL")?;
    let rows = stmt
        .query_map([], from_row)?
        .collect::<rusqlite::Result<_>>();
    rows.context("reading trial_merges rows")
}

pub fn in_flight_features(conn: &Connection, primary: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, branch FROM features
         WHERE state IN ('open', 'ready') AND primary_branch = ?1 AND branch IS NOT NULL
         ORDER BY created_at, id",
    )?;
    let rows = stmt
        .query_map([primary], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>();
    rows.context("reading the in-flight features")
}
