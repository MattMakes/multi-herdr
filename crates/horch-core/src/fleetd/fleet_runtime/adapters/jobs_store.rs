//! The `jobs` table: its rows, with no rule.
//!
//! The `state` column changes only through `transition()`
//! (`application::ledger`). Each other column is written here.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::taskledger::util;

/// 1 row of `jobs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    pub id: i64,
    pub kind: Kind,
    pub subject: String,
    pub repo: Option<String>,
    pub input_key: String,
    pub run: i64,
    pub abandoned_runs: u64,
    pub not_before: Option<FleetSeconds>,
    pub state: State,
    pub pid: Option<u32>,
    pub pid_start: Option<u64>,
    pub time_limit_s: u64,
    pub deadline: Option<FleetSeconds>,
    pub result_json: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
}

const COLUMNS: &str = "id, kind, subject, repo, input_key, run, abandoned_runs, not_before,
    state, pid, pid_start, time_limit_s, deadline, result_json, created_at, started_at, ended_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<JobRow> {
    let word = |i: usize, w: String| {
        rusqlite::Error::FromSqlConversionFailure(
            i,
            rusqlite::types::Type::Text,
            format!("unknown word {w}").into(),
        )
    };
    let kind: String = r.get(1)?;
    let state: String = r.get(8)?;
    Ok(JobRow {
        id: r.get(0)?,
        kind: Kind::parse(&kind).ok_or_else(|| word(1, kind))?,
        subject: r.get(2)?,
        repo: r.get(3)?,
        input_key: r.get(4)?,
        run: r.get(5)?,
        abandoned_runs: r.get(6)?,
        not_before: r.get::<_, Option<i64>>(7)?.map(FleetSeconds),
        state: State::parse(&state).ok_or_else(|| word(8, state))?,
        pid: r.get(9)?,
        pid_start: r.get(10)?,
        time_limit_s: r.get(11)?,
        deadline: r.get::<_, Option<i64>>(12)?.map(FleetSeconds),
        result_json: r.get(13)?,
        created_at: r.get(14)?,
        started_at: r.get(15)?,
        ended_at: r.get(16)?,
    })
}

fn rows(conn: &Connection, filter: &str, p: impl rusqlite::Params) -> Result<Vec<JobRow>> {
    let sql = format!("SELECT {COLUMNS} FROM jobs {filter}");
    let mut stmt = conn.prepare(&sql)?;
    let out = stmt
        .query_map(p, from_row)?
        .collect::<rusqlite::Result<_>>();
    out.context("reading jobs rows")
}

fn one(conn: &Connection, filter: &str, p: impl rusqlite::Params) -> Result<Option<JobRow>> {
    let sql = format!("SELECT {COLUMNS} FROM jobs {filter}");
    let row = conn.query_row(&sql, p, from_row).optional();
    row.context("reading a jobs row")
}

/// The row `id`.
pub fn get(conn: &Connection, id: i64) -> Result<Option<JobRow>> {
    one(conn, "WHERE id = ?1", [id])
}

/// The `queued` or `running` run of `kind` and `subject` (at most 1, by
/// the index `jobs_one_active`).
pub fn active(conn: &Connection, kind: Kind, subject: &str) -> Result<Option<JobRow>> {
    one(
        conn,
        "WHERE kind = ?1 AND subject = ?2 AND state IN ('queued', 'running')",
        params![kind.as_str(), subject],
    )
}

/// The last run of `kind`, `subject` and `input_key`.
pub fn latest(
    conn: &Connection,
    kind: Kind,
    subject: &str,
    input_key: &str,
) -> Result<Option<JobRow>> {
    one(
        conn,
        "WHERE kind = ?1 AND subject = ?2 AND input_key = ?3 ORDER BY run DESC LIMIT 1",
        params![kind.as_str(), subject, input_key],
    )
}

/// The `queued` runs that may start at `clock`, oldest first.
pub fn due(conn: &Connection, clock: FleetSeconds) -> Result<Vec<JobRow>> {
    rows(
        conn,
        "WHERE state = 'queued' AND (not_before IS NULL OR not_before <= ?1) ORDER BY id",
        [clock.0],
    )
}

/// The `running` runs, oldest first.
pub fn running(conn: &Connection) -> Result<Vec<JobRow>> {
    rows(conn, "WHERE state = 'running' ORDER BY id", [])
}

/// A new `queued` run.
#[derive(Debug, Clone)]
pub struct NewRun<'a> {
    pub kind: Kind,
    pub subject: &'a str,
    pub repo: Option<&'a str>,
    pub input_key: &'a str,
    pub run: i64,
    pub abandoned_runs: u64,
    pub not_before: Option<FleetSeconds>,
    pub time_limit_s: u64,
}

/// Insert `new` as a `queued` row. Gives its id.
pub fn insert(conn: &Connection, new: &NewRun<'_>) -> Result<i64> {
    conn.execute(
        "INSERT INTO jobs (kind, subject, repo, input_key, run, abandoned_runs, not_before,
             state, time_limit_s, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8, ?9)",
        params![
            new.kind.as_str(),
            new.subject,
            new.repo,
            new.input_key,
            new.run,
            new.abandoned_runs,
            new.not_before.map(|t| t.0),
            new.time_limit_s,
            util::now(),
        ],
    )
    .with_context(|| format!("queueing a {} job of {}", new.kind, new.subject))?;
    Ok(conn.last_insert_rowid())
}

/// Record the process of the run `id` and its deadline.
pub fn set_started(
    conn: &Connection,
    id: i64,
    pid: u32,
    pid_start: Option<u64>,
    deadline: FleetSeconds,
) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET pid = ?2, pid_start = ?3, deadline = ?4, started_at = ?5 WHERE id = ?1",
        params![id, pid, pid_start, deadline.0, util::now()],
    )?;
    Ok(())
}

/// Record the result of the run `id`.
pub fn set_result(conn: &Connection, id: i64, result_json: &str) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET result_json = ?2 WHERE id = ?1",
        params![id, result_json],
    )?;
    Ok(())
}

/// Record the end of the run `id` with `abandoned_runs`.
pub fn set_ended(conn: &Connection, id: i64, abandoned_runs: u64) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET ended_at = ?2, abandoned_runs = ?3 WHERE id = ?1",
        params![id, util::now(), abandoned_runs],
    )?;
    Ok(())
}
