//! The `jobs` table: its rows, with no rule.
//!
//! The `state` column changes only through `transition()`
//! (`application::ledger`). Each other column is written here.

use anyhow::{Context, Result};
use rusqlite::types::Type;
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

/// The row `r`, read by column name.
fn from_row(r: &Row<'_>) -> rusqlite::Result<JobRow> {
    let word = |column: &str, w: String| {
        rusqlite::Error::InvalidColumnType(0, format!("{column} = {w}"), Type::Text)
    };
    let kind: String = r.get("kind")?;
    let state: String = r.get("state")?;
    Ok(JobRow {
        id: r.get("id")?,
        kind: Kind::parse(&kind).ok_or_else(|| word("kind", kind))?,
        subject: r.get("subject")?,
        repo: r.get("repo")?,
        input_key: r.get("input_key")?,
        run: r.get("run")?,
        abandoned_runs: r.get("abandoned_runs")?,
        not_before: r.get::<_, Option<i64>>("not_before")?.map(FleetSeconds),
        state: State::parse(&state).ok_or_else(|| word("state", state))?,
        pid: r.get("pid")?,
        pid_start: r.get("pid_start")?,
        time_limit_s: r.get("time_limit_s")?,
        deadline: r.get::<_, Option<i64>>("deadline")?.map(FleetSeconds),
        result_json: r.get("result_json")?,
        created_at: r.get("created_at")?,
        started_at: r.get("started_at")?,
        ended_at: r.get("ended_at")?,
    })
}

fn rows(conn: &Connection, filter: &str, p: impl rusqlite::Params) -> Result<Vec<JobRow>> {
    let sql = format!("SELECT * FROM jobs {filter}");
    let mut stmt = conn.prepare(&sql)?;
    let out = stmt
        .query_map(p, from_row)?
        .collect::<rusqlite::Result<_>>();
    out.context("reading jobs rows")
}

fn one(conn: &Connection, filter: &str, p: impl rusqlite::Params) -> Result<Option<JobRow>> {
    let sql = format!("SELECT * FROM jobs {filter}");
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

/// The newest run of `kind` and `subject`, of any input key.
pub fn last(conn: &Connection, kind: Kind, subject: &str) -> Result<Option<JobRow>> {
    one(
        conn,
        "WHERE kind = ?1 AND subject = ?2 ORDER BY id DESC LIMIT 1",
        params![kind.as_str(), subject],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::schema;

    /// Each column gets a distinct value, so a column read into the wrong
    /// field fails.
    #[test]
    fn each_column_goes_to_its_field() {
        let mut conn = Connection::open_in_memory().unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        conn.execute(
            "INSERT INTO jobs (id, kind, subject, repo, input_key, run, abandoned_runs,
                not_before, state, pid, pid_start, time_limit_s, deadline, result_json,
                created_at, started_at, ended_at)
             VALUES (7, 'push', 'F-1', '/repo', 'tips', 2, 3, 40, 'running', 5, 6, 600, 90,
                '{}', 'c', 's', 'e')",
            [],
        )
        .unwrap();
        let row = get(&conn, 7).unwrap().unwrap();
        let want = JobRow {
            id: 7,
            kind: Kind::Push,
            subject: "F-1".into(),
            repo: Some("/repo".into()),
            input_key: "tips".into(),
            run: 2,
            abandoned_runs: 3,
            not_before: Some(FleetSeconds(40)),
            state: State::Running,
            pid: Some(5),
            pid_start: Some(6),
            time_limit_s: 600,
            deadline: Some(FleetSeconds(90)),
            result_json: Some("{}".into()),
            created_at: "c".into(),
            started_at: Some("s".into()),
            ended_at: Some("e".into()),
        };
        assert_eq!(row, want);
    }

    #[test]
    fn last_is_the_newest_run_of_a_kind_and_subject() {
        let mut conn = Connection::open_in_memory().unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        let run = |subject, input_key, run| NewRun {
            kind: Kind::TaskCheck,
            subject,
            repo: None,
            input_key,
            run,
            abandoned_runs: 0,
            not_before: None,
            time_limit_s: 60,
        };
        assert_eq!(last(&conn, Kind::TaskCheck, "T-1").unwrap(), None);
        let first = insert(&conn, &run("T-1", "tip1", 1)).unwrap();
        // 1 run of a kind and subject is active at a time.
        conn.execute("UPDATE jobs SET state = 'abandoned' WHERE id = ?1", [first])
            .unwrap();
        let newest = insert(&conn, &run("T-1", "tip2", 1)).unwrap();
        insert(&conn, &run("T-2", "tip3", 1)).unwrap();
        let row = last(&conn, Kind::TaskCheck, "T-1").unwrap().unwrap();
        assert_eq!((row.id, row.input_key.as_str()), (newest, "tip2"));
        assert_eq!(last(&conn, Kind::Push, "T-1").unwrap(), None);
    }
}
