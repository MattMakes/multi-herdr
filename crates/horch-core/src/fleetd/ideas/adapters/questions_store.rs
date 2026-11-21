use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::fleetd::ideas::domain::questions::{Question, State};

const COLUMNS: &str = "id, task_id, feature_id, owner, state, due_at, asked_at, answered_at";

fn question(r: &Row<'_>) -> rusqlite::Result<Question> {
    let word: String = r.get(4)?;
    let state = State::parse(&word).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            anyhow!("questions.state {word:?}").into(),
        )
    })?;
    Ok(Question {
        id: r.get(0)?,
        task_id: r.get(1)?,
        feature_id: r.get(2)?,
        owner: r.get(3)?,
        state,
        due_at: r.get(5)?,
        asked_at: r.get(6)?,
        answered_at: r.get(7)?,
    })
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Question>> {
    let sql = format!("SELECT {COLUMNS} FROM questions WHERE id = ?1");
    Ok(conn.query_row(&sql, [id], question).optional()?)
}

pub fn insert(conn: &Connection, q: &Question) -> Result<()> {
    let sql = format!("INSERT INTO questions ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)");
    conn.execute(
        &sql,
        params![
            q.id,
            q.task_id,
            q.feature_id,
            q.owner,
            q.state.as_str(),
            q.due_at,
            q.asked_at,
            q.answered_at
        ],
    )?;
    Ok(())
}

pub fn set_answered(conn: &Connection, id: &str, at: &str) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE questions SET state = ?2, answered_at = ?3 WHERE id = ?1 AND state IN (?4, ?5)",
        params![
            id,
            State::Answered.as_str(),
            at,
            State::Open.as_str(),
            State::Escalated.as_str()
        ],
    )? > 0)
}

pub fn set_escalated(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE questions SET state = ?2 WHERE id = ?1 AND state = ?3",
        params![id, State::Escalated.as_str(), State::Open.as_str()],
    )? > 0)
}

pub fn set_owner(conn: &Connection, id: &str, feature_id: &str, owner: &str) -> Result<()> {
    conn.execute(
        "UPDATE questions SET feature_id = ?2, owner = ?3 WHERE id = ?1",
        params![id, feature_id, owner],
    )?;
    Ok(())
}

pub fn delay_open_of_feature(conn: &Connection, feature_id: &str, by_s: i64) -> Result<()> {
    conn.execute(
        "UPDATE questions SET due_at = due_at + ?2 WHERE feature_id = ?1 AND state = ?3",
        params![feature_id, by_s, State::Open.as_str()],
    )?;
    Ok(())
}

pub fn move_open(conn: &Connection, from: &str, to: &str, owner: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE questions SET feature_id = ?2, owner = ?3 WHERE feature_id = ?1 AND state = ?4",
        params![from, to, owner, State::Open.as_str()],
    )?)
}

pub fn ids_of_attempt(conn: &Connection, attempt_id: &str) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT id FROM questions WHERE substr(id, 1, length(?1) + 1) = ?1 || '/'")?;
    let rows = stmt.query_map([attempt_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn answered_of_attempt(conn: &Connection, attempt_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM questions
         WHERE state = 'answered' AND substr(id, 1, length(?1) + 1) = ?1 || '/')",
        [attempt_id],
        |r| r.get(0),
    )?)
}

pub fn open_of_task(conn: &Connection, task_id: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT count(*) FROM questions WHERE task_id = ?1 AND state = ?2",
        params![task_id, State::Open.as_str()],
        |r| r.get(0),
    )?)
}

pub fn unanswered_of_task(conn: &Connection, task_id: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT count(*) FROM questions WHERE task_id = ?1 AND state <> ?2",
        params![task_id, State::Answered.as_str()],
        |r| r.get(0),
    )?)
}

pub fn open(conn: &Connection) -> Result<Vec<Question>> {
    rows_where(conn, "state = ?1", [State::Open.as_str()])
}

pub fn unanswered(conn: &Connection) -> Result<Vec<Question>> {
    rows_where(conn, "state <> ?1", [State::Answered.as_str()])
}

pub fn of_task(conn: &Connection, task_id: &str) -> Result<Vec<Question>> {
    rows_where(conn, "task_id = ?1", [task_id])
}

pub fn of_tasks<'a>(
    conn: &Connection,
    task_ids: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<Question>> {
    let ids: Vec<&str> = task_ids.into_iter().collect();
    let sql = format!(
        "SELECT {COLUMNS} FROM questions WHERE task_id IN ({}) ORDER BY asked_at, rowid",
        crate::taskledger::sql_words::marks(1, ids.len())
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids), question)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn rows_where(conn: &Connection, filter: &str, args: [&str; 1]) -> Result<Vec<Question>> {
    let sql = format!("SELECT {COLUMNS} FROM questions WHERE {filter} ORDER BY asked_at, rowid");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(args, question)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
