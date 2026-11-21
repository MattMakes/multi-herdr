use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::fleetd::task_execution::domain::task::{
    AttemptRow, AttemptState, Recorded, Rows, TaskRow, WorkerRow,
};
use crate::taskledger::model::{TaskStatus, WorkerKind, WorkerStatus};
use crate::taskledger::runs;
use crate::taskledger::store::one_snapshot;

fn recorded(pid: Option<i64>, start: Option<i64>) -> Option<Recorded> {
    let pid = u32::try_from(pid?).ok().filter(|p| *p > 0)?;
    Some(Recorded {
        pid,
        pid_start: start.and_then(|s| u64::try_from(s).ok()),
    })
}

const ATTEMPT_COLUMNS: &str = "a.id, a.task_id, a.worker_id, a.state,
    COALESCE(a.pid, w.agent_pid), CASE WHEN a.pid IS NULL THEN w.agent_started ELSE a.pid_start END,
    COALESCE(a.mux_handle, w.pane_id)";

fn attempt(r: &Row<'_>) -> rusqlite::Result<AttemptRow> {
    let word: String = r.get(3)?;
    let state = AttemptState::parse(&word).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            anyhow!("attempts.state {word:?}").into(),
        )
    })?;
    Ok(AttemptRow {
        id: r.get(0)?,
        task_id: r.get(1)?,
        worker_id: r.get(2)?,
        state,
        process: recorded(r.get(4)?, r.get(5)?),
        pane: r.get(6)?,
    })
}

fn attempts_where(conn: &Connection, filter: &str, args: &[&str]) -> Result<Vec<AttemptRow>> {
    let sql = format!(
        "SELECT {ATTEMPT_COLUMNS} FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
         {filter} ORDER BY a.rowid"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), attempt)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn attempts_of_worker(conn: &Connection, worker_id: &str) -> Result<Vec<AttemptRow>> {
    attempts_where(conn, "WHERE a.worker_id = ?1", &[worker_id])
}

pub fn open_attempts_with_persona(conn: &Connection) -> Result<Vec<(AttemptRow, Option<String>)>> {
    let sql = format!(
        "SELECT {ATTEMPT_COLUMNS}, w.persona FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.state != 'ended' AND COALESCE(a.pid, w.agent_pid) IS NOT NULL ORDER BY a.rowid"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| Ok((attempt(r)?, r.get(7)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptProcesses {
    pub attempt: String,
    pub task: Option<String>,
    pub persona: Option<String>,
    pub ended: bool,
    pub processes: Vec<Recorded>,
}

pub fn fleet_processes(conn: &Connection, ended_since: &str) -> Result<Vec<AttemptProcesses>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.task_id, w.persona, a.state = 'ended', a.pid, a.pid_start,
                w.supervisor_pid, w.supervisor_started, w.agent_pid, w.agent_started
         FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.state != 'ended' OR a.ended_at >= ?1 ORDER BY a.rowid",
    )?;
    let rows = stmt.query_map([ended_since], |r| {
        let processes = [(4, 5), (6, 7), (8, 9)]
            .into_iter()
            .map(|(p, s)| Ok(recorded(r.get(p)?, r.get(s)?)))
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(AttemptProcesses {
            attempt: r.get(0)?,
            task: r.get(1)?,
            persona: r.get(2)?,
            ended: r.get(3)?,
            processes,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub type AttemptVia = (Option<String>, String, String, Option<String>);

pub fn attempt_via(conn: &Connection, id: &str) -> Result<Option<AttemptVia>> {
    let row = conn
        .query_row(
            "SELECT a.task_id, a.state, a.via, w.persona
             FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
             WHERE a.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(row)
}

pub fn worker_of_attempt(conn: &Connection, id: &str) -> Result<Option<String>> {
    let row = conn
        .query_row("SELECT worker_id FROM attempts WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(row)
}

pub fn tasks_in(conn: &Connection, status: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT task_id FROM tasks WHERE status = ?1 ORDER BY task_id")?;
    let ids = stmt
        .query_map([status], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

fn worker(r: &Row<'_>, host: &str) -> rusqlite::Result<WorkerRow> {
    let status: String = r.get(2)?;
    let kind: String = r.get(1)?;
    let row_host: Option<String> = r.get(3)?;
    let live = WorkerStatus::parse(&status).is_ok_and(WorkerStatus::is_live);
    Ok(WorkerRow {
        id: r.get(0)?,
        live,
        orchestrator: kind == WorkerKind::Orchestrator.as_str(),
        here: row_host.is_none_or(|h| h == host),
        agent: recorded(r.get(4)?, r.get(5)?),
        supervisor: recorded(r.get(6)?, r.get(7)?),
        pane: r.get(8)?,
        status,
    })
}

fn workers(conn: &Connection, host: &str) -> Result<BTreeMap<String, WorkerRow>> {
    let mut stmt = conn.prepare(
        "SELECT worker_id, kind, status, host, agent_pid, agent_started, supervisor_pid,
            supervisor_started, pane_id
         FROM workers",
    )?;
    let rows = stmt.query_map([], |r| worker(r, host))?;
    let mut out = BTreeMap::new();
    for w in rows {
        let w = w?;
        out.insert(w.id.clone(), w);
    }
    Ok(out)
}

pub fn task_states_of_idea(conn: &Connection, idea_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt =
        conn.prepare("SELECT task_id, status FROM tasks WHERE idea_id = ?1 ORDER BY rowid")?;
    let rows = stmt.query_map([idea_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn idea_has_task_in(conn: &Connection, idea_id: &str, status: TaskStatus) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM tasks WHERE idea_id = ?1 AND status = ?2)",
        [idea_id, status.as_str()],
        |r| r.get(0),
    )?)
}

pub fn ideas_of_tasks<'a>(
    conn: &Connection,
    task_ids: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeMap<String, String>> {
    let ids: Vec<&str> = task_ids.into_iter().collect();
    let sql = format!(
        "SELECT task_id, idea_id FROM tasks WHERE idea_id IS NOT NULL AND task_id IN ({})",
        crate::taskledger::sql_words::marks(1, ids.len())
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids), |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn worker_personas(conn: &Connection) -> Result<BTreeMap<String, Option<String>>> {
    let mut stmt = conn.prepare("SELECT worker_id, persona FROM workers")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn outcomes_of<'a>(
    conn: &Connection,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeMap<String, Option<String>>> {
    let ids: Vec<&str> = ids.into_iter().collect();
    let sql = format!(
        "SELECT id, outcome FROM attempts WHERE id IN ({})",
        crate::taskledger::sql_words::marks(1, ids.len())
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids), |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OwnerFacts {
    pub attempt_states: BTreeMap<String, String>,
    pub worker_statuses: BTreeMap<String, String>,
    pub tasks: BTreeMap<String, (String, Option<String>)>,
}

pub fn owner_facts<'a>(
    conn: &Connection,
    attempts: impl IntoIterator<Item = &'a str>,
    workers: impl IntoIterator<Item = &'a str>,
    tasks: impl IntoIterator<Item = &'a str>,
) -> Result<OwnerFacts> {
    fn by_id<T>(
        conn: &Connection,
        sql: &str,
        ids: Vec<&str>,
        row: impl Fn(&Row<'_>) -> rusqlite::Result<(String, T)>,
    ) -> Result<BTreeMap<String, T>> {
        let sql = sql.replace(
            "{marks}",
            &crate::taskledger::sql_words::marks(1, ids.len()),
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(ids), row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    let pair = |r: &Row<'_>| Ok((r.get(0)?, r.get(1)?));
    Ok(OwnerFacts {
        attempt_states: by_id(
            conn,
            "SELECT id, state FROM attempts WHERE id IN ({marks})",
            attempts.into_iter().collect(),
            pair,
        )?,
        worker_statuses: by_id(
            conn,
            "SELECT worker_id, status FROM workers WHERE worker_id IN ({marks})",
            workers.into_iter().collect(),
            pair,
        )?,
        tasks: by_id(
            conn,
            "SELECT task_id, status, held_reason FROM tasks WHERE task_id IN ({marks})",
            tasks.into_iter().collect(),
            |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))),
        )?,
    })
}

fn live_tasks(conn: &Connection, attempts: &[AttemptRow]) -> Result<BTreeMap<String, TaskRow>> {
    let mut stmt = conn.prepare(
        "SELECT task_id, status, current_worker_id, attempts - attempt_floor, run_id
         FROM tasks
         WHERE task_id IN (SELECT task_id FROM attempts WHERE state = 'live')",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let status: String = r.get(1)?;
            let holds = [TaskStatus::Running, TaskStatus::Verifying]
                .iter()
                .any(|s| s.as_str() == status);
            let held: Option<String> = r.get(2)?;
            let row = TaskRow {
                id: r.get(0)?,
                held_by: held.filter(|_| holds),
                used: r.get(3)?,
                max_attempts: 0,
            };
            Ok((row, r.get::<_, String>(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let runs = runs::of_ids(conn, rows.iter().map(|(_, run)| run.as_str()))?;
    let mut out = BTreeMap::new();
    for (mut t, run) in rows {
        let Some(run) = runs.get(&run) else {
            continue;
        };
        t.max_attempts = run.max_attempts;
        out.insert(t.id.clone(), t);
    }
    let live = attempts
        .iter()
        .filter(|a| a.state == AttemptState::Live)
        .filter_map(|a| a.task_id.as_deref());
    for t in live {
        if !out.contains_key(t) {
            return Err(anyhow!("no task {t} with its run"));
        }
    }
    Ok(out)
}

pub fn rows(conn: &Connection, host: &str) -> Result<Rows> {
    one_snapshot(conn, || {
        let attempts = attempts_where(conn, "", &[])?;
        let tasks = live_tasks(conn, &attempts)?;
        Ok(Rows {
            attempts,
            workers: workers(conn, host)?,
            tasks,
        })
    })
}

pub fn insert_attempt(conn: &Connection, a: &AttemptRow, now: &str) -> Result<()> {
    let p = a.process;
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, mux_handle, pid, pid_start, state, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            a.id,
            a.task_id,
            a.worker_id,
            a.pane,
            p.map(|p| i64::from(p.pid)),
            p.and_then(|p| p.pid_start).and_then(|s| i64::try_from(s).ok()),
            a.state.as_str(),
            now
        ],
    )?;
    Ok(())
}

pub fn set_process(conn: &Connection, id: &str, p: Recorded) -> Result<()> {
    conn.execute(
        "UPDATE attempts SET pid = ?2, pid_start = ?3 WHERE id = ?1",
        params![
            id,
            i64::from(p.pid),
            p.pid_start.and_then(|s| i64::try_from(s).ok())
        ],
    )?;
    Ok(())
}

pub fn process_of_other_attempt(conn: &Connection, id: &str, p: Recorded) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM attempts WHERE id != ?1 AND pid = ?2 AND pid_start = ?3",
        params![
            id,
            i64::from(p.pid),
            p.pid_start.and_then(|s| i64::try_from(s).ok())
        ],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub fn set_missing_process(conn: &Connection, id: &str, p: Recorded) -> Result<bool> {
    let n = conn.execute(
        "UPDATE attempts SET pid = ?2, pid_start = ?3 WHERE id = ?1 AND pid IS NULL",
        params![
            id,
            i64::from(p.pid),
            p.pid_start.and_then(|s| i64::try_from(s).ok())
        ],
    )?;
    Ok(n == 1)
}

pub fn set_ended(conn: &Connection, id: &str, outcome: &str, now: &str) -> Result<()> {
    conn.execute(
        "UPDATE attempts SET outcome = ?2, ended_at = COALESCE(ended_at, ?3) WHERE id = ?1",
        params![id, outcome, now],
    )?;
    Ok(())
}

pub fn open_tasks_of_idea(conn: &Connection, idea_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT task_id FROM tasks WHERE idea_id = ?1 AND status NOT IN (?2, ?3)
         ORDER BY task_id",
    )?;
    let ended = [TaskStatus::Done.as_str(), TaskStatus::Cancelled.as_str()];
    let ids = stmt
        .query_map(params![idea_id, ended[0], ended[1]], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

pub fn tasks_of_idea(conn: &Connection, idea_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT task_id FROM tasks WHERE idea_id = ?1 ORDER BY task_id")?;
    let ids = stmt
        .query_map([idea_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

pub fn move_tasks_of_idea(conn: &Connection, idea_id: &str, run_id: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE tasks SET run_id = ?2, updated_at = ?3 WHERE idea_id = ?1",
        params![idea_id, run_id, crate::taskledger::util::now()],
    )?)
}

pub fn idea_of_task(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let idea = conn.query_row(
        "SELECT idea_id FROM tasks WHERE task_id = ?1",
        [task_id],
        |r| r.get(0),
    )?;
    Ok(idea)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveWrapper {
    pub id: String,
    pub task_id: Option<String>,
    pub idea_id: Option<String>,
    pub pid: u32,
    pub pid_start: u64,
}

pub fn live_wrappers(conn: &Connection) -> Result<Vec<LiveWrapper>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, idea_id, pid, pid_start FROM attempts
         WHERE state = 'live' AND pid IS NOT NULL AND pid_start IS NOT NULL
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            recorded(r.get(3)?, r.get(4)?),
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, task_id, idea_id, process) = row?;
        if let Some(Recorded {
            pid,
            pid_start: Some(pid_start),
        }) = process
        {
            out.push(LiveWrapper {
                id,
                task_id,
                idea_id,
                pid,
                pid_start,
            });
        }
    }
    Ok(out)
}

pub fn task_and_idea(
    conn: &Connection,
    id: &str,
) -> Result<Option<(Option<String>, Option<String>)>> {
    use rusqlite::OptionalExtension as _;
    Ok(conn
        .query_row(
            "SELECT task_id, idea_id FROM attempts WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

pub fn idea_attempts(
    conn: &Connection,
    idea_id: &str,
) -> Result<Vec<(AttemptRow, Option<String>)>> {
    let sql = format!(
        "SELECT {ATTEMPT_COLUMNS}, a.outcome FROM attempts a
         JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.idea_id = ?1 AND a.task_id IS NULL ORDER BY a.rowid"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([idea_id], |r| Ok((attempt(r)?, r.get(7)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn ideas_with_open_attempts(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT idea_id FROM attempts
         WHERE task_id IS NULL AND idea_id IS NOT NULL AND state <> 'ended' ORDER BY 1",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn set_idea(conn: &Connection, id: &str, idea_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE attempts SET idea_id = ?2, via = 'push' WHERE id = ?1",
        params![id, idea_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::store::Store;

    fn ledger(tasks: &[(&str, &str, i64, i64, &str)]) -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("ledger.sqlite3")).unwrap();
        store
            .immediate(|tx| {
                tx.execute_batch(
                    "INSERT INTO projects (project_id, root, created_at) VALUES ('p1', '/p', 'now');
                     INSERT INTO runs (run_id, project_id, slug, finish_steps, status, started_at,
                         max_attempts)
                     VALUES ('r1', 'p1', 'r1', '[]', 'active', 'now', 5);",
                )?;
                let idea = crate::taskledger::schema::run_idea(tx, "r1", "now", true)?;
                for (id, status, attempts, floor, state) in tasks {
                    let worker = format!("W-{id}");
                    tx.execute(
                        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona,
                             claimable_by, status, attempts, attempt_floor, current_worker_id,
                             created_at, updated_at, idea_id)
                         VALUES (?1, 'r1', (SELECT count(*) + 1 FROM tasks), ?1, 'a.md', 'opus',
                             'opus', ?2, ?3, ?4, ?5, 'now', 'now', ?6)",
                        params![id, status, attempts, floor, worker, idea],
                    )?;
                    tx.execute(
                        "INSERT INTO workers (worker_id, run_id, persona, host, status, task_id)
                         VALUES (?1, 'r1', 'opus', 'h', 'working', ?2)",
                        params![worker, id],
                    )?;
                    tx.execute(
                        "INSERT INTO attempts (id, task_id, worker_id, state)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![format!("{id}/A1"), id, worker, state],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        (tmp, store)
    }

    #[test]
    fn rows_reads_the_task_of_each_live_attempt() {
        let (_tmp, store) = ledger(&[
            ("A", "running", 3, 1, "live"),
            ("B", "verifying", 1, 0, "live"),
            ("C", "stalled", 2, 0, "live"),
            ("D", "running", 1, 0, "ended"),
            ("E", "running", 1, 0, "launching"),
        ]);
        let rows = rows(store.conn(), "h").unwrap();
        let row = |id: &str, held: Option<&str>, used| TaskRow {
            id: id.to_string(),
            held_by: held.map(str::to_string),
            used,
            max_attempts: 5,
        };
        let want: BTreeMap<String, TaskRow> = [
            row("A", Some("W-A"), 2),
            row("B", Some("W-B"), 1),
            row("C", None, 2),
        ]
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect();
        assert_eq!(rows.tasks, want);
        assert_eq!(rows.attempts.len(), 5);
        assert_eq!(rows.workers.len(), 5);
    }

    #[test]
    fn rows_runs_the_same_statements_for_3_and_300_live_tasks() {
        let count = |n: usize| {
            let ids: Vec<String> = (0..n).map(|i| format!("T{i:04}")).collect();
            let tasks: Vec<(&str, &str, i64, i64, &str)> = ids
                .iter()
                .map(|id| (id.as_str(), "running", 1, 0, "live"))
                .collect();
            let (_tmp, store) = ledger(&tasks);
            let (rows, statements) =
                crate::taskledger::testing::count_statements(store.conn(), || {
                    rows(store.conn(), "h").unwrap()
                });
            assert_eq!(rows.tasks.len(), n);
            statements
        };
        assert_eq!(count(3), count(300));
    }
}
