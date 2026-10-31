//! Worker rows: 1 process life each (section 4.6).

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Row};
use serde::Serialize;

use super::model::{Actor, EventKind, Origin, ProgressSource, WorkerKind, WorkerStatus};
use super::sql_words::{self, word};
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

/// What spawn launched: role, harness, model and effort.
pub fn set_launch(
    conn: &Connection,
    worker_id: &str,
    role: &str,
    harness: &str,
    model: &str,
    effort: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE workers SET role = COALESCE(role, ?2), harness = ?3, model = ?4, effort = ?5
         WHERE worker_id = ?1",
        params![worker_id, role, harness, model, effort],
    )?;
    Ok(())
}

/// The spawn failed before `horch worker` ran.
pub fn launch_failed(conn: &Connection, worker_id: &str, reason: &str) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE workers SET status = ?2, end_reason = ?3, ended_at = ?4
             WHERE worker_id = ?1 AND status IN ({unclaimed})",
            unclaimed = sql_words::list(sql_words::WORKERS_UNCLAIMED)
        ),
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
    let n = conn.execute(
        &format!(
            "UPDATE workers SET agent_pid = ?2, agent_started = ?3,
                status = CASE WHEN status = {starting} THEN {claiming} ELSE status END,
                last_checkin_at = ?4
             WHERE worker_id = ?1",
            starting = word(WorkerStatus::Starting),
            claiming = word(WorkerStatus::Claiming)
        ),
        params![
            worker_id,
            pid as i64,
            started.map(|s| s as i64),
            util::now()
        ],
    )?;
    if n == 0 {
        bail!("no worker {worker_id} in the task ledger");
    }
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
    if source == ProgressSource::Agent {
        if let Some(reason) = text.trim_start().strip_prefix("waiting:") {
            set_wait(conn, worker_id, "background", reason.trim(), &[], 24 * 3600)?;
        } else {
            clear_wait(conn, worker_id, "background")?;
        }
    }
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
        &format!(
            "UPDATE workers SET status = ?2, end_reason = COALESCE(end_reason, ?3),
                exit_code = COALESCE(?4, exit_code), ended_at = COALESCE(ended_at, ?5)
             WHERE worker_id = ?1 AND status IN ({live})",
            live = sql_words::list(sql_words::WORKERS_LIVE)
        ),
        params![worker_id, status.as_str(), reason, exit_code, util::now()],
    )?;
    Ok(n > 0)
}

/// How long a `spawning` row may wait for its supervisor: longer than the
/// registration wait of `horch spawn` (#190).
pub const LAUNCH_GRACE_S: i64 = 120;

/// How long a live `claiming` worker may take to claim before the
/// orchestrator is asked to look at its pane (#190).
pub const CLAIM_GRACE_S: i64 = 600;

/// Whether a pre-claim row (`spawning`, `starting`, `claiming`) no longer
/// stands for a process on its way to a claim: its last checkin is older
/// than `heartbeat_stale_s`, or it never checked in within
/// [`LAUNCH_GRACE_S`] of its spawn.
pub fn is_stale(w: &Worker, heartbeat_stale_s: i64, now: &str) -> bool {
    if !matches!(w.status.as_str(), "spawning" | "starting" | "claiming") {
        return false;
    }
    let older = |t: Option<&str>, limit: i64| {
        t.and_then(|t| util::seconds_between(t, now))
            .is_some_and(|age| age > limit)
    };
    match w.last_checkin_at.as_deref() {
        Some(t) => older(Some(t), heartbeat_stale_s),
        None => older(w.spawned_at.as_deref(), LAUNCH_GRACE_S),
    }
}

/// Whether the worker's recorded supervisor process still runs.
pub fn supervisor_alive(w: &Worker) -> bool {
    w.supervisor_pid
        .is_some_and(|p| crate::procid::alive(p as u32, w.supervisor_started.map(|s| s as u64)))
}

/// What the pre-claim sweep did to 1 worker.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Preclaim {
    pub worker: Worker,
    /// The status it ended with; `None` for a live worker that has not
    /// claimed within [`CLAIM_GRACE_S`] (reported once).
    pub ended: Option<String>,
    pub note: String,
}

const NO_CLAIM: &str = "No claim:";

/// The watchdog's pass over pre-claim rows (#190). A row whose process is
/// gone or silent ends: `launch_failed` before the agent ran, `no_task`
/// after. A live `claiming` worker past [`CLAIM_GRACE_S`] is reported once.
pub fn sweep_preclaim(
    conn: &Connection,
    run: &super::runs::Run,
    alive: &dyn Fn(&Worker) -> bool,
) -> Result<Vec<Preclaim>> {
    let now = util::now();
    let mut out = Vec::new();
    for w in of_run(conn, &run.run_id)? {
        if w.kind != WorkerKind::Worker.as_str() {
            continue;
        }
        let status = w.status()?;
        if !matches!(
            status,
            WorkerStatus::Spawning | WorkerStatus::Starting | WorkerStatus::Claiming
        ) {
            continue;
        }
        let about = About {
            run: Some(&run.run_id),
            task: None,
            worker: Some(&w.worker_id),
        };
        let gone = w.supervisor_pid.is_some() && !alive(&w);
        if gone || is_stale(&w, run.heartbeat_stale_s, &now) {
            let (end_as, why) = match (status, gone) {
                (WorkerStatus::Claiming, true) => (
                    WorkerStatus::NoTask,
                    "its supervisor is gone before it claimed a task",
                ),
                (WorkerStatus::Claiming, false) => {
                    (WorkerStatus::NoTask, "no checkin before it claimed a task")
                }
                (_, true) => (
                    WorkerStatus::LaunchFailed,
                    "its supervisor is gone before the agent ran",
                ),
                (_, false) => (
                    WorkerStatus::LaunchFailed,
                    "no horch worker checked in after the spawn",
                ),
            };
            if end(conn, &w.worker_id, end_as, why, None)? {
                util::event(
                    conn,
                    &about,
                    Actor::Watchdog,
                    EventKind::Exited,
                    &format!("{} ended {end_as}: {why}.", w.label()),
                )?;
                out.push(Preclaim {
                    worker: w,
                    ended: Some(end_as.as_str().to_string()),
                    note: why.to_string(),
                });
            }
            continue;
        }
        let waited = w
            .started_at
            .as_deref()
            .or(w.spawned_at.as_deref())
            .and_then(|t| util::seconds_between(t, &now))
            .unwrap_or(0);
        if status != WorkerStatus::Claiming || waited <= CLAIM_GRACE_S {
            continue;
        }
        let told: i64 = conn.query_row(
            "SELECT count(*) FROM events WHERE worker_id = ?1 AND kind = 'verdict' AND text LIKE ?2",
            params![w.worker_id, format!("{NO_CLAIM}%")],
            |r| r.get(0),
        )?;
        if told > 0 {
            continue;
        }
        let note = format!("{} has not claimed a task in {}s", w.label(), waited);
        util::event(
            conn,
            &about,
            Actor::Watchdog,
            EventKind::Verdict,
            &format!("{NO_CLAIM} {note}."),
        )?;
        out.push(Preclaim {
            worker: w,
            ended: None,
            note,
        });
    }
    Ok(out)
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

/// A known bounded wait; multiple causes may coexist for one worker.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Wait {
    pub kind: String,
    pub reason: String,
    pub started_at: String,
    pub deadline: String,
    pub task_ids: Vec<String>,
}

/// The wait kind of an author on its reviews.
pub const REVIEWS_WAIT: &str = "reviewers";

/// How long an author waits for its reviews when no deadline is known.
pub const REVIEWS_WAIT_SECS: i64 = 7 * 24 * 3600;

/// Whether `wait` is an author's reviewers wait after its reviews are in:
/// it has no task ids, so [`waiting`] keeps it until finish clears it or
/// its deadline passes (#177).
pub fn holds_for_finish(wait: &Wait) -> bool {
    wait.kind == REVIEWS_WAIT && wait.task_ids.is_empty()
}

/// After the last review of its task: keep `worker`'s reviewers wait, with
/// no task ids, up to the deadline it had (#177). With no earlier wait, the
/// wait lasts [`REVIEWS_WAIT_SECS`]. A deadline that has passed sets no
/// wait: the stall checks then judge the author.
pub fn hold_for_finish(conn: &Connection, worker: &str) -> Result<()> {
    // Read the row, not `waiting`: that ends a reviewers wait whose reviews
    // are done, which they all are now.
    let deadline: Option<String> = optional(conn.query_row(
        "SELECT deadline FROM worker_waits WHERE worker_id = ?1 AND kind = ?2",
        params![worker, REVIEWS_WAIT],
        |r| r.get(0),
    ))?;
    let left = match deadline {
        None => REVIEWS_WAIT_SECS,
        Some(raw) => {
            let at = crate::clock::parse(&raw)
                .with_context(|| format!("the reviewers wait of {worker} has deadline {raw:?}"))?;
            (at - crate::clock::now()).num_seconds()
        }
    };
    if left <= 0 {
        return clear_wait(conn, worker, REVIEWS_WAIT);
    }
    set_wait(
        conn,
        worker,
        REVIEWS_WAIT,
        "the reviews are in; waiting for horch ledger finish",
        &[],
        left,
    )
}

pub fn set_wait(
    conn: &Connection,
    worker: &str,
    kind: &str,
    reason: &str,
    task_ids: &[String],
    seconds: i64,
) -> Result<()> {
    let now = crate::clock::now();
    let deadline = crate::clock::stamp(now + chrono::Duration::seconds(seconds));
    conn.execute("INSERT INTO worker_waits(worker_id,kind,reason,started_at,deadline,task_ids)
        VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(worker_id,kind) DO UPDATE SET
        reason=excluded.reason,started_at=excluded.started_at,deadline=excluded.deadline,task_ids=excluded.task_ids",
        params![worker,kind,reason,crate::clock::stamp(now),deadline,serde_json::to_string(task_ids)?])?;
    conn.execute(
        "UPDATE workers SET stall_rounds=0 WHERE worker_id=?1",
        [worker],
    )?;
    Ok(())
}

pub fn clear_wait(conn: &Connection, worker: &str, kind: &str) -> Result<()> {
    if conn.execute(
        "DELETE FROM worker_waits WHERE worker_id=?1 AND kind=?2",
        params![worker, kind],
    )? > 0
    {
        conn.execute(
            "UPDATE workers SET stall_rounds=0 WHERE worker_id=?1",
            [worker],
        )?;
    }
    Ok(())
}

/// Expired waits and completed reviews stop holding the watchdog immediately.
pub fn waiting(conn: &Connection, worker: &str) -> Result<Vec<Wait>> {
    let mut stmt=conn.prepare("SELECT kind,reason,started_at,deadline,task_ids FROM worker_waits WHERE worker_id=?1 ORDER BY kind")?;
    let rows = stmt
        .query_map([worker], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for (kind, reason, started_at, deadline, ids) in rows {
        let task_ids: Vec<String> = serde_json::from_str(&ids)?;
        let reviews_done = kind == REVIEWS_WAIT
            && !task_ids.is_empty()
            && task_ids.iter().all(|id| {
                super::tasks::get(conn, id)
                    .is_ok_and(|t| matches!(t.status.as_str(), "done" | "cancelled"))
            });
        if deadline <= util::now() || reviews_done {
            clear_wait(conn, worker, &kind)?;
            continue;
        }
        out.push(Wait {
            kind,
            reason,
            started_at,
            deadline,
            task_ids,
        });
    }
    Ok(out)
}

/// Project the messaging layer's authoritative questions into ledger waits.
pub fn sync_questions(conn: &Connection, state: &std::path::Path, workspace: &str) -> Result<()> {
    let questions = crate::messaging::mail::Mail::under(state, workspace).open_questions()?;
    let mut stmt = conn.prepare(&format!(
        "SELECT * FROM workers WHERE workspace_id=?1 AND status={working}",
        working = word(WorkerStatus::Working)
    ))?;
    let roster = stmt
        .query_map([workspace], Worker::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for w in roster {
        // The first unanswered question fixes the deadline; polling must never extend it.
        let question = questions
            .iter()
            .find(|q| w.role.as_deref() == Some(q.from.as_str()));
        if let Some(q) = question {
            let started: chrono::DateTime<chrono::Utc> = q.created_at.into();
            let deadline = crate::clock::stamp(started + chrono::Duration::days(7));
            conn.execute("INSERT INTO worker_waits(worker_id,kind,reason,started_at,deadline) VALUES(?1,'question',?2,?3,?4)
                ON CONFLICT(worker_id,kind) DO UPDATE SET reason=excluded.reason,started_at=excluded.started_at,deadline=excluded.deadline",
                params![w.worker_id,q.text,crate::clock::stamp(started),deadline])?;
        } else {
            clear_wait(conn, &w.worker_id, "question")?;
        }
    }
    Ok(())
}

pub fn role_state(conn: &Connection, workspace: &str, role: &str) -> Result<String> {
    let w = optional(conn.query_row(
        "SELECT * FROM workers WHERE workspace_id=?1 AND role=?2 ORDER BY rowid DESC LIMIT 1",
        params![workspace, role],
        Worker::from_row,
    ))?;
    let Some(w) = w else {
        return Ok(String::new());
    };
    let verdict: Option<String> = optional(conn.query_row(
        "SELECT verdict FROM checks WHERE worker_id=?1 ORDER BY check_id DESC LIMIT 1",
        [&w.worker_id],
        |r| r.get(0),
    ))?;
    let waits = waiting(conn, &w.worker_id)?;
    Ok(format!(
        " watchdog={}{}",
        verdict.as_deref().unwrap_or("unchecked"),
        waits
            .iter()
            .map(|w| format!(
                " waiting: {} ({}s ago)",
                w.reason,
                util::seconds_between(&w.started_at, &util::now()).unwrap_or(0)
            ))
            .collect::<String>()
    ))
}

/// Join the native worker life to its execution record for session and compaction hooks.
pub fn set_record(conn: &Connection, worker_id: &str, record_id: &str) -> Result<()> {
    let changed = conn.execute(
        "UPDATE workers SET legacy_record_id=?2 WHERE worker_id=?1",
        params![worker_id, record_id],
    )?;
    if changed == 0 {
        bail!("no worker {worker_id} in the task ledger");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::store::Store;

    fn author(store: &mut Store) -> String {
        store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/r"),
                    &NewRun {
                        slug: "s".into(),
                        ..Default::default()
                    },
                )?;
                insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id,
                        persona: "opus".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap()
    }

    /// An agent PID for a worker the ledger does not know is an error, as
    /// for the supervisor PID. It used to exit 0 and change nothing.
    #[test]
    fn agent_started_refuses_an_unknown_worker() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let err = agent_started(s.conn(), "w-missing", 4243, Some(2)).unwrap_err();
        assert_eq!(err.to_string(), "no worker w-missing in the task ledger");
    }

    /// The agent PID of a known worker is stored and the worker moves on to
    /// claiming.
    #[test]
    fn agent_started_records_the_pid_and_moves_a_starting_worker_on() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let id = author(&mut s);
        supervisor_started(s.conn(), &id, 4242, Some(1)).unwrap();

        agent_started(s.conn(), &id, 4243, Some(2)).unwrap();

        let w = get(s.conn(), &id).unwrap();
        assert_eq!(w.agent_pid, Some(4243));
        assert_eq!(w.status().unwrap(), WorkerStatus::Claiming);
    }

    fn seconds_left(wait: &Wait) -> i64 {
        (crate::clock::parse(&wait.deadline).unwrap() - crate::clock::now()).num_seconds()
    }

    /// #177: the reviews are in. The author keeps waiting, with no task
    /// ids, up to the deadline it had.
    #[test]
    fn a_held_wait_keeps_its_deadline_and_survives_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let id = author(&mut s);
        set_wait(
            s.conn(),
            &id,
            REVIEWS_WAIT,
            "reviews",
            &["T-9".into()],
            7200,
        )
        .unwrap();

        hold_for_finish(s.conn(), &id).unwrap();

        let waits = waiting(s.conn(), &id).unwrap();
        assert_eq!(waits.len(), 1, "{waits:?}");
        assert!(holds_for_finish(&waits[0]), "{waits:?}");
        assert!(
            (7195..=7200).contains(&seconds_left(&waits[0])),
            "{waits:?}"
        );
    }

    #[test]
    fn a_held_wait_with_no_earlier_wait_lasts_7_days() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let id = author(&mut s);

        hold_for_finish(s.conn(), &id).unwrap();

        let waits = waiting(s.conn(), &id).unwrap();
        assert_eq!(waits.len(), 1, "{waits:?}");
        let left = seconds_left(&waits[0]);
        assert!(
            (7 * 24 * 3600 - 5..=7 * 24 * 3600).contains(&left),
            "{left}"
        );
    }

    /// A deadline that has passed is not renewed: the stall checks judge
    /// the author at once.
    #[test]
    fn a_wait_past_its_deadline_is_not_held() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let id = author(&mut s);
        set_wait(s.conn(), &id, REVIEWS_WAIT, "reviews", &["T-9".into()], -60).unwrap();

        hold_for_finish(s.conn(), &id).unwrap();

        let rows: i64 = s
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM worker_waits WHERE worker_id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0);
    }
}
