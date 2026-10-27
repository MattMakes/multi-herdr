//! Worker rows: 1 process life each (section 4.6).

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Row};
use serde::Serialize;

use super::model::{Actor, EventKind, Origin, ProgressSource, WorkerKind, WorkerStatus};
use super::store::optional;
use super::util::{self, About};

/// 1 worker row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Worker {
    pub worker_id: String,
    pub run_id: Option<String>,
    pub kind: String,
    pub persona: Option<String>,
    pub role: Option<String>,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub host: Option<String>,
    pub workspace_id: Option<String>,
    pub pane_id: Option<String>,
    pub supervisor_pid: Option<i64>,
    pub supervisor_started: Option<i64>,
    pub agent_pid: Option<i64>,
    pub agent_started: Option<i64>,
    pub previous_worker_id: Option<String>,
    pub resumed_from_session: Option<String>,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub legacy_record_id: Option<String>,
    pub last_checkin_at: Option<String>,
    pub last_progress_at: Option<String>,
    pub last_progress_source: Option<String>,
    pub stall_rounds: i64,
    pub task_id: Option<String>,
    pub attempt: Option<i64>,
    pub status: String,
    pub exit_code: Option<i64>,
    pub end_reason: Option<String>,
    pub summary: Option<String>,
    pub origin: String,
    pub spawned_at: Option<String>,
    pub started_at: Option<String>,
    pub claimed_at: Option<String>,
    pub ended_at: Option<String>,
}

impl Worker {
    pub fn status(&self) -> Result<WorkerStatus> {
        WorkerStatus::parse(&self.status)
    }

    /// The name used in messages: the role, else the id.
    pub fn label(&self) -> &str {
        self.role.as_deref().unwrap_or(&self.worker_id)
    }

    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Worker> {
        Ok(Worker {
            worker_id: r.get("worker_id")?,
            run_id: r.get("run_id")?,
            kind: r.get("kind")?,
            persona: r.get("persona")?,
            role: r.get("role")?,
            harness: r.get("harness")?,
            model: r.get("model")?,
            effort: r.get("effort")?,
            host: r.get("host")?,
            workspace_id: r.get("workspace_id")?,
            pane_id: r.get("pane_id")?,
            supervisor_pid: r.get("supervisor_pid")?,
            supervisor_started: r.get("supervisor_started")?,
            agent_pid: r.get("agent_pid")?,
            agent_started: r.get("agent_started")?,
            previous_worker_id: r.get("previous_worker_id")?,
            resumed_from_session: r.get("resumed_from_session")?,
            session_id: r.get("session_id")?,
            transcript_path: r.get("transcript_path")?,
            legacy_record_id: r.get("legacy_record_id")?,
            last_checkin_at: r.get("last_checkin_at")?,
            last_progress_at: r.get("last_progress_at")?,
            last_progress_source: r.get("last_progress_source")?,
            stall_rounds: r.get("stall_rounds")?,
            task_id: r.get("task_id")?,
            attempt: r.get("attempt")?,
            status: r.get("status")?,
            exit_code: r.get("exit_code")?,
            end_reason: r.get("end_reason")?,
            summary: r.get("summary")?,
            origin: r.get("origin")?,
            spawned_at: r.get("spawned_at")?,
            started_at: r.get("started_at")?,
            claimed_at: r.get("claimed_at")?,
            ended_at: r.get("ended_at")?,
        })
    }
}

/// 1 worker by id.
pub fn get(conn: &Connection, worker_id: &str) -> Result<Worker> {
    find(conn, worker_id)?.with_context(|| format!("no worker {worker_id} in the task ledger"))
}

/// 1 worker by id, if it exists.
pub fn find(conn: &Connection, worker_id: &str) -> Result<Option<Worker>> {
    optional(conn.query_row(
        "SELECT * FROM workers WHERE worker_id = ?1",
        [worker_id],
        Worker::from_row,
    ))
}

/// A worker by its id or its role in `run_id`: the newest row with that role.
pub fn resolve(conn: &Connection, run_id: &str, key: &str) -> Result<Worker> {
    if let Some(w) = find(conn, key)? {
        return Ok(w);
    }
    optional(conn.query_row(
        "SELECT * FROM workers WHERE run_id = ?1 AND role = ?2
         ORDER BY spawned_at DESC, rowid DESC LIMIT 1",
        params![run_id, key],
        Worker::from_row,
    ))?
    .with_context(|| format!("no worker '{key}' in this run"))
}

/// Every worker of a run, oldest first.
pub fn of_run(conn: &Connection, run_id: &str) -> Result<Vec<Worker>> {
    let mut stmt =
        conn.prepare("SELECT * FROM workers WHERE run_id = ?1 ORDER BY spawned_at, rowid")?;
    let rows = stmt
        .query_map([run_id], Worker::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The attempts at a task, oldest first.
pub fn attempts_at(conn: &Connection, task_id: &str) -> Result<Vec<Worker>> {
    let mut stmt = conn
        .prepare("SELECT * FROM workers WHERE task_id = ?1 ORDER BY attempt, claimed_at, rowid")?;
    let rows = stmt
        .query_map([task_id], Worker::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// What spawn records before the pane opens.
#[derive(Debug, Clone, Default)]
pub struct NewWorker {
    pub run_id: String,
    pub kind: Option<WorkerKind>,
    pub persona: String,
    pub role: Option<String>,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub workspace_id: Option<String>,
    pub pane_id: Option<String>,
    pub previous_worker_id: Option<String>,
    pub resumed_from_session: Option<String>,
    pub legacy_record_id: Option<String>,
}

/// Insert a worker in state `spawning` and return its id.
pub fn insert(conn: &Connection, new: &NewWorker) -> Result<String> {
    if let Some(prev) = &new.previous_worker_id {
        let p = get(conn, prev)?;
        if p.run_id.as_deref() != Some(new.run_id.as_str()) {
            bail!("worker {prev} belongs to another run");
        }
    }
    let id = util::new_id();
    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, role, harness, model, effort,
            host, workspace_id, pane_id, previous_worker_id, resumed_from_session,
            legacy_record_id, status, origin, spawned_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            id,
            new.run_id,
            new.kind.unwrap_or(WorkerKind::Worker).as_str(),
            new.persona,
            new.role,
            new.harness,
            new.model,
            new.effort,
            util::host_name(),
            new.workspace_id,
            new.pane_id,
            new.previous_worker_id,
            new.resumed_from_session,
            new.legacy_record_id,
            WorkerStatus::Spawning.as_str(),
            Origin::Native.as_str(),
            util::now(),
        ],
    )?;
    Ok(id)
}

/// Record the pane spawn opened.
pub fn set_pane(conn: &Connection, worker_id: &str, pane_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE workers SET pane_id = ?2 WHERE worker_id = ?1",
        params![worker_id, pane_id],
    )?;
    Ok(())
}

/// The spawn failed before `horch worker` ran.
pub fn launch_failed(conn: &Connection, worker_id: &str, reason: &str) -> Result<()> {
    conn.execute(
        "UPDATE workers SET status = ?2, end_reason = ?3, ended_at = ?4
         WHERE worker_id = ?1 AND status IN ('spawning', 'starting')",
        params![
            worker_id,
            WorkerStatus::LaunchFailed.as_str(),
            reason,
            util::now()
        ],
    )?;
    Ok(())
}

/// `horch worker` runs: its own PID and start time.
pub fn supervisor_started(
    conn: &Connection,
    worker_id: &str,
    pid: u32,
    started: Option<u64>,
) -> Result<()> {
    let now = util::now();
    let n = conn.execute(
        "UPDATE workers SET supervisor_pid = ?2, supervisor_started = ?3, status = ?4,
            started_at = ?5, last_checkin_at = ?5
         WHERE worker_id = ?1",
        params![
            worker_id,
            pid as i64,
            started.map(|s| s as i64),
            WorkerStatus::Starting.as_str(),
            now
        ],
    )?;
    if n == 0 {
        bail!("no worker {worker_id} in the task ledger");
    }
    Ok(())
}

/// The agent child started: its PID and start time. The worker now looks
/// for a task.
pub fn agent_started(
    conn: &Connection,
    worker_id: &str,
    pid: u32,
    started: Option<u64>,
) -> Result<()> {
    conn.execute(
        "UPDATE workers SET agent_pid = ?2, agent_started = ?3,
            status = CASE WHEN status = 'starting' THEN 'claiming' ELSE status END,
            last_checkin_at = ?4
         WHERE worker_id = ?1",
        params![
            worker_id,
            pid as i64,
            started.map(|s| s as i64),
            util::now()
        ],
    )?;
    Ok(())
}

/// The heartbeat: the supervisor is alive now.
pub fn checkin(conn: &Connection, worker_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE workers SET last_checkin_at = ?2 WHERE worker_id = ?1",
        params![worker_id, util::now()],
    )?;
    Ok(())
}

/// The session id and transcript, once known. Only fills what is empty, and
/// says whether it changed anything.
pub fn set_session(
    conn: &Connection,
    worker_id: &str,
    session_id: Option<&str>,
    transcript: Option<&str>,
) -> Result<bool> {
    let n = conn.execute(
        "UPDATE workers SET
            session_id = COALESCE(session_id, ?2),
            transcript_path = COALESCE(transcript_path, ?3)
         WHERE worker_id = ?1
           AND ((session_id IS NULL AND ?2 IS NOT NULL)
             OR (transcript_path IS NULL AND ?3 IS NOT NULL))",
        params![worker_id, session_id, transcript],
    )?;
    if n > 0 {
        let w = get(conn, worker_id)?;
        util::event(
            conn,
            &About {
                run: w.run_id.as_deref(),
                task: w.task_id.as_deref(),
                worker: Some(worker_id),
            },
            Actor::Supervisor,
            EventKind::Session,
            &format!(
                "Session {} recorded. Transcript: {}.",
                w.session_id.as_deref().unwrap_or("(unknown)"),
                w.transcript_path.as_deref().unwrap_or("(unknown)")
            ),
        )?;
    }
    Ok(n > 0)
}

/// Progress seen for a worker: from the agent itself, or a proxy checkin
/// from the watchdog. Resets the stall count.
pub fn progress(
    conn: &Connection,
    worker_id: &str,
    source: ProgressSource,
    kind: EventKind,
    text: &str,
) -> Result<()> {
    let w = get(conn, worker_id)?;
    conn.execute(
        "UPDATE workers SET last_progress_at = ?2, last_progress_source = ?3, stall_rounds = 0
         WHERE worker_id = ?1",
        params![worker_id, util::now(), source.as_str()],
    )?;
    let actor = match source {
        ProgressSource::Agent => Actor::Worker,
        _ => Actor::Watchdog,
    };
    util::event(
        conn,
        &About {
            run: w.run_id.as_deref(),
            task: w.task_id.as_deref(),
            worker: Some(worker_id),
        },
        actor,
        kind,
        text,
    )?;
    Ok(())
}

/// Set a worker's final state, unless it already ended.
pub fn end(
    conn: &Connection,
    worker_id: &str,
    status: WorkerStatus,
    reason: &str,
    exit_code: Option<i32>,
) -> Result<bool> {
    let n = conn.execute(
        "UPDATE workers SET status = ?2, end_reason = COALESCE(end_reason, ?3),
            exit_code = COALESCE(?4, exit_code), ended_at = COALESCE(ended_at, ?5)
         WHERE worker_id = ?1 AND status IN ('spawning', 'starting', 'claiming', 'working')",
        params![worker_id, status.as_str(), reason, exit_code, util::now()],
    )?;
    Ok(n > 0)
}

/// Record the exit code of a worker that already ended (a `done` worker).
pub fn set_exit_code(conn: &Connection, worker_id: &str, exit_code: Option<i32>) -> Result<()> {
    conn.execute(
        "UPDATE workers SET exit_code = COALESCE(?2, exit_code), ended_at = COALESCE(ended_at, ?3)
         WHERE worker_id = ?1",
        params![worker_id, exit_code, util::now()],
    )?;
    Ok(())
}
