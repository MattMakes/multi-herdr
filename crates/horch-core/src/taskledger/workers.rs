//! Worker rows: 1 process life each (section 4.6).

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use super::model::{
    Actor, EndReason, EventKind, Origin, ProgressSource, StepStatus, TaskKind, TaskStatus,
    WorkerKind, WorkerStatus,
};
use super::sql_words::{self, word};
use super::store::optional;
use super::util::{self, About};
use crate::execution::lifecycle::TRANSCRIPT_JSONL;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::questions_store;
use crate::fleetd::ideas::application::questions;
use crate::fleetd::ideas::domain::questions::{Question, State as QuestionState};
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::adapters::task_files::attempt_dir;
use crate::fleetd::task_execution::adapters::waits_store::{self, WaitRow};
use crate::fleetd::task_execution::application::lease;
use crate::fleetd::task_execution::domain::waits::{
    self, Behind, Child, End, Facts, Proof, Verdict,
};
use crate::fleetd::worker_profiles::domain::profile::{Liveness, Mode};
use crate::fleetd::worktree_custody::adapters::executors::fleet_root;

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
    /// The handoff file of a worker that ended `handed_off` (C4).
    pub handoff_path: Option<String>,
    /// The task this worker was spawned to claim (TRF-24). The claim takes
    /// it first; no other worker claims it while this worker is live.
    pub target_task_id: Option<String>,
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
            handoff_path: r.get("handoff_path")?,
            target_task_id: r.get("target_task_id")?,
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
    /// The task this worker is spawned to claim (TRF-24).
    pub target_task_id: Option<String>,
}

/// Insert a worker in state `spawning` and return its id.
pub fn insert(conn: &Connection, new: &NewWorker) -> Result<String> {
    if let Some(prev) = &new.previous_worker_id {
        let p = get(conn, prev)?;
        if p.run_id.as_deref() != Some(new.run_id.as_str()) {
            bail!("worker {prev} belongs to another run");
        }
    }
    if let Some(target) = &new.target_task_id {
        let run: Option<String> = optional(conn.query_row(
            "SELECT run_id FROM tasks WHERE task_id = ?1",
            [target],
            |r| r.get(0),
        ))?;
        if run.as_deref() != Some(new.run_id.as_str()) {
            bail!("task {target} is not a task of this run");
        }
    }
    let id = util::new_id();
    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, role, harness, model, effort,
            host, workspace_id, pane_id, previous_worker_id, resumed_from_session,
            legacy_record_id, status, origin, spawned_at, target_task_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
            new.target_task_id,
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
    util::atomic(conn, |conn| {
        let unclaimed = find(conn, worker_id)?
            .map(|w| w.status())
            .transpose()?
            .is_some_and(|s| sql_words::WORKERS_UNCLAIMED.contains(&s));
        if !unclaimed {
            return Ok(());
        }
        util::transition(
            conn,
            Entity::Worker,
            worker_id,
            WorkerStatus::LaunchFailed.as_str(),
            Actor::Horch,
            util::Why::Rule(util::Rule::LaunchFailed, reason),
        )?;
        conn.execute(
            "UPDATE workers SET end_reason = ?2, ended_at = ?3 WHERE worker_id = ?1",
            params![worker_id, reason, util::now()],
        )?;
        Ok(())
    })
}

/// `horch worker` runs: its own PID and start time.
pub fn supervisor_started(
    conn: &Connection,
    worker_id: &str,
    pid: u32,
    started: Option<u64>,
) -> Result<()> {
    util::atomic(conn, |conn| {
        if find(conn, worker_id)?.is_none() {
            bail!("no worker {worker_id} in the task ledger");
        }
        util::transition(
            conn,
            Entity::Worker,
            worker_id,
            WorkerStatus::Starting.as_str(),
            Actor::Supervisor,
            util::Why::Rule(
                util::Rule::SupervisorStarted,
                &format!("horch worker runs as PID {pid}"),
            ),
        )?;
        conn.execute(
            "UPDATE workers SET supervisor_pid = ?2, supervisor_started = ?3,
                started_at = ?4, last_checkin_at = ?4
             WHERE worker_id = ?1",
            params![
                worker_id,
                pid as i64,
                started.map(|s| s as i64),
                util::now()
            ],
        )?;
        Ok(())
    })
}

/// The agent child started: its PID and start time. The worker now looks
/// for a task.
pub fn agent_started(
    conn: &Connection,
    worker_id: &str,
    pid: u32,
    started: Option<u64>,
) -> Result<()> {
    util::atomic(conn, |conn| {
        let Some(w) = find(conn, worker_id)? else {
            bail!("no worker {worker_id} in the task ledger");
        };
        if w.status()? == WorkerStatus::Starting {
            util::transition(
                conn,
                Entity::Worker,
                worker_id,
                WorkerStatus::Claiming.as_str(),
                Actor::Supervisor,
                util::Why::Rule(
                    util::Rule::AgentStarted,
                    &format!("the agent runs as PID {pid}"),
                ),
            )?;
        }
        conn.execute(
            "UPDATE workers SET agent_pid = ?2, agent_started = ?3, last_checkin_at = ?4
             WHERE worker_id = ?1",
            params![
                worker_id,
                pid as i64,
                started.map(|s| s as i64),
                util::now()
            ],
        )?;
        Ok(())
    })
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
            set_wait(
                conn,
                worker_id,
                BACKGROUND_WAIT,
                reason.trim(),
                &[],
                24 * 3600,
            )?;
        } else {
            clear_wait(conn, worker_id, BACKGROUND_WAIT)?;
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

/// Set a worker's final state, unless it already ended. The cause of the
/// change is `end: <reason>`.
pub fn end(
    conn: &Connection,
    worker_id: &str,
    status: WorkerStatus,
    reason: &str,
    exit_code: Option<i32>,
) -> Result<bool> {
    let why = util::Why::Rule(util::Rule::End, reason);
    end_by(
        conn,
        worker_id,
        status,
        reason,
        exit_code,
        Actor::Horch,
        why,
    )
}

/// [`end`] with the actor and the cause of the change.
fn end_by(
    conn: &Connection,
    worker_id: &str,
    status: WorkerStatus,
    reason: &str,
    exit_code: Option<i32>,
    actor: Actor,
    why: util::Why<'_>,
) -> Result<bool> {
    util::atomic(conn, |conn| {
        if !is_live(conn, worker_id)? {
            return Ok(false);
        }
        util::transition(conn, Entity::Worker, worker_id, status.as_str(), actor, why)?;
        conn.execute(
            "UPDATE workers SET end_reason = COALESCE(end_reason, ?2),
                exit_code = COALESCE(?3, exit_code), ended_at = COALESCE(ended_at, ?4)
             WHERE worker_id = ?1",
            params![worker_id, reason, exit_code, util::now()],
        )?;
        Ok(true)
    })
}

/// Whether the worker `worker_id` exists and is live.
fn is_live(conn: &Connection, worker_id: &str) -> Result<bool> {
    Ok(find(conn, worker_id)?
        .map(|w| w.status())
        .transpose()?
        .is_some_and(|s| sql_words::WORKERS_LIVE.contains(&s)))
}

/// End a live worker `handed_off` with its handoff file (trace-fixes C4).
/// The attempt did not finish, so the row ends `incomplete`, as a reaped
/// one does: `plan-workers` then names it for `--replaces`.
pub fn hand_off(conn: &Connection, worker_id: &str, path: &str) -> Result<()> {
    util::atomic(conn, |conn| {
        if !is_live(conn, worker_id)? {
            bail!("worker {worker_id} already ended; an ended worker cannot hand off");
        }
        util::transition(
            conn,
            Entity::Worker,
            worker_id,
            WorkerStatus::Incomplete.as_str(),
            Actor::Worker,
            util::Why::Rule(util::Rule::Handoff, path),
        )?;
        conn.execute(
            "UPDATE workers SET end_reason = ?2, handoff_path = ?3,
                ended_at = COALESCE(ended_at, ?4)
             WHERE worker_id = ?1",
            params![worker_id, EndReason::HandedOff.as_str(), path, util::now()],
        )?;
        Ok(())
    })
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
    sweep_preclaim_of(conn, run, of_run(conn, &run.run_id)?, alive)
}

/// [`sweep_preclaim`] of the rows `rows` of `run` only: the fleetd pass
/// sweeps 1 page of rows in each pass (PL-5).
pub fn sweep_preclaim_of(
    conn: &Connection,
    run: &super::runs::Run,
    rows: Vec<Worker>,
    alive: &dyn Fn(&Worker) -> bool,
) -> Result<Vec<Preclaim>> {
    let now = util::now();
    let mut out = Vec::new();
    for w in rows {
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
            let text = format!("{} ended {end_as}: {why}.", w.label());
            let cause = util::Why::Event(EventKind::Exited, &text);
            if end_by(
                conn,
                &w.worker_id,
                end_as,
                why,
                None,
                Actor::Watchdog,
                cause,
            )? {
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
    /// The fleet clock at which the wait stops counting as activity
    /// (`waiting_until`): its cap `max_wait_s` (FM-3).
    pub due_at: i64,
    /// The `questions` row behind a question wait.
    pub question_id: Option<String>,
}

/// The wait kind of an author on its reviews.
pub const REVIEWS_WAIT: &str = "reviewers";

/// The wait kind of a worker question.
pub const QUESTION_WAIT: &str = "question";

/// The wait kind of a long command or a detached finish.
pub const BACKGROUND_WAIT: &str = "background";

/// How long an author waits for its reviews when no deadline is known.
pub const REVIEWS_WAIT_SECS: i64 = 7 * 24 * 3600;

/// `liveness.max_wait_s` of a worker. A ledger command has no roster, so it
/// uses the default of a profile, as the claim does for `lease_s`.
fn max_wait_s() -> i64 {
    i64::try_from(Liveness::default().max_wait_s).unwrap_or(i64::MAX)
}

/// Whether `wait` is an author's reviewers wait after its reviews are in:
/// it has no task ids, so [`waiting`] keeps it until finish clears it or
/// its deadline or its cap passes (#177).
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

/// Set the wait `kind` of `worker`: its calendar deadline `seconds` from
/// now, and its cap `due_at` [`max_wait_s`] after the fleet clock.
/// The mode of the newest attempt of `worker`, as its launch fixed it: exec
/// when its attempt folder holds the exec stream ([`TRANSCRIPT_JSONL`]),
/// which the wrapper opens before it starts the CLI. The profile of today
/// does not change it, so a live interactive attempt stays interactive
/// after an update. An unknown fact (no attempt, no fleet root) gives
/// interactive, the treatment of the release before.
pub fn attempt_mode(conn: &Connection, worker: &str) -> Mode {
    let attempt = conn
        .query_row(
            "SELECT id, task_id FROM attempts WHERE worker_id = ?1 AND task_id IS NOT NULL
             ORDER BY rowid DESC LIMIT 1",
            [worker],
            |r| {
                Ok(AttemptRef {
                    attempt_id: r.get(0)?,
                    task_id: r.get(1)?,
                })
            },
        )
        .optional();
    match (attempt, fleet_root(conn)) {
        (Ok(Some(a)), Ok(root)) => Mode::of_launch(
            attempt_dir(&root, &a.task_id, a.attempt())
                .join(TRANSCRIPT_JSONL)
                .exists(),
        ),
        _ => Mode::Interactive,
    }
}

pub fn set_wait(
    conn: &Connection,
    worker: &str,
    kind: &str,
    reason: &str,
    task_ids: &[String],
    seconds: i64,
) -> Result<()> {
    if kind == BACKGROUND_WAIT && !attempt_mode(conn, worker).has_background_wait() {
        return Ok(());
    }
    let now = crate::clock::now();
    let clock = lease::fleet_clock(conn)?.0;
    let row = WaitRow {
        kind: kind.to_string(),
        reason: reason.to_string(),
        started_at: crate::clock::stamp(now),
        deadline: crate::clock::stamp(now + chrono::Duration::seconds(seconds)),
        task_ids: serde_json::to_string(task_ids)?,
        due_at: Some(clock.saturating_add(max_wait_s())),
        question_id: None,
    };
    waits_store::put(conn, worker, &row)?;
    conn.execute(
        "UPDATE workers SET stall_rounds=0 WHERE worker_id=?1",
        [worker],
    )?;
    Ok(())
}

pub fn clear_wait(conn: &Connection, worker: &str, kind: &str) -> Result<()> {
    if waits_store::delete(conn, worker, kind)? {
        conn.execute(
            "UPDATE workers SET stall_rounds=0 WHERE worker_id=?1",
            [worker],
        )?;
    }
    Ok(())
}

/// End the wait `kind` of `worker` for `cause`: 1 `wait_ended` event, then
/// the clear.
fn end_wait(conn: &Connection, w: &Worker, kind: &str, reason: &str, cause: End) -> Result<()> {
    util::event(
        conn,
        &About {
            run: w.run_id.as_deref(),
            task: w.task_id.as_deref(),
            worker: Some(&w.worker_id),
        },
        Actor::Horch,
        EventKind::WaitEnded,
        &format!("{}: the {kind} wait ended ({reason})", cause.as_str()),
    )?;
    clear_wait(conn, &w.worker_id, kind)
}

/// The states of a review task that does work: a proof behind a wait.
const REVIEW_WORKING: [TaskStatus; 3] =
    [TaskStatus::Ready, TaskStatus::Leased, TaskStatus::Running];

/// The child fact of the agent of `w`: a member of its process group other
/// than the agent is alive. Another host, a platform with no process
/// groups, or no recorded agent pid gives [`Child::Unknown`]: a missing pid
/// is no proof that the process is gone, so the wait holds to its cap.
fn agent_child(w: &Worker) -> Child {
    if w.host.as_deref().is_some_and(|h| h != util::host_name()) {
        return Child::Unknown;
    }
    let Some(pid) = w.agent_pid.and_then(|p| u32::try_from(p).ok()) else {
        return Child::Unknown;
    };
    let started = w.agent_started.and_then(|s| u64::try_from(s).ok());
    if !crate::procid::alive(pid, started) {
        return Child::Gone;
    }
    match horch_platform::process::group_members(pid) {
        Ok(members) if members.iter().any(|m| *m != pid) => Child::Alive,
        Ok(_) => Child::Gone,
        Err(_) => Child::Unknown,
    }
}

/// The proof of work behind the background wait of `w`.
fn background_proof(conn: &Connection, w: &Worker) -> Result<Proof> {
    let Some(task) = w.task_id.as_deref() else {
        return Ok(Proof {
            child: agent_child(w),
            step_running: false,
            review_live: false,
        });
    };
    let step_running: i64 = conn.query_row(
        &format!(
            "SELECT count(*) FROM task_steps WHERE task_id = ?1 AND worker_id = ?2 AND status = {}",
            word(StepStatus::Running)
        ),
        params![task, w.worker_id],
        |r| r.get(0),
    )?;
    let review_live: i64 = conn.query_row(
        &format!(
            "SELECT count(*) FROM tasks WHERE parent_task_id = ?1 AND kind = {}
             AND status IN ({})",
            word(TaskKind::Review),
            sql_words::list(&REVIEW_WORKING)
        ),
        [task],
        |r| r.get(0),
    )?;
    Ok(Proof {
        child: agent_child(w),
        step_running: step_running > 0,
        review_live: review_live > 0,
    })
}

/// What is behind the wait `row` of `w`.
fn behind(conn: &Connection, w: &Worker, row: &WaitRow, task_ids: &[String]) -> Result<Behind> {
    Ok(match row.kind.as_str() {
        // A question wait that no sync linked yet (a wait of the release)
        // holds to its cap.
        QUESTION_WAIT => Behind::Question {
            open: match &row.question_id {
                None => true,
                Some(id) => questions_store::get(conn, id)?
                    .is_some_and(|q| q.state != QuestionState::Answered),
            },
        },
        REVIEWS_WAIT if task_ids.is_empty() => Behind::HeldForFinish,
        REVIEWS_WAIT => {
            let states = task_ids
                .iter()
                .map(|id| super::tasks::get(conn, id).map(|t| t.status))
                .collect::<Result<Vec<_>>>()?;
            Behind::Reviews {
                working: states
                    .iter()
                    .any(|s| REVIEW_WORKING.iter().any(|w| w.as_str() == s)),
                ended: states
                    .iter()
                    .all(|s| matches!(s.as_str(), "done" | "cancelled")),
            }
        }
        _ => Behind::Background(background_proof(conn, w)?),
    })
}

/// The waits of `worker` that count as activity now. A wait past its
/// deadline or its cap `due_at`, or with no live row behind it, ends with a
/// `wait_ended` event. A wait with a proof of work gets its cap moved
/// ([`waits::judge`]).
pub fn waiting(conn: &Connection, worker: &str) -> Result<Vec<Wait>> {
    let rows = waits_store::of_worker(conn, worker)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let w = get(conn, worker)?;
    let facts = Facts {
        clock: lease::fleet_clock(conn)?.0,
        max_wait_s: max_wait_s(),
    };
    let now = util::now();
    let mut out = Vec::new();
    for row in rows {
        let task_ids: Vec<String> = serde_json::from_str(&row.task_ids)?;
        let due_at = row
            .due_at
            .unwrap_or_else(|| facts.clock.saturating_add(facts.max_wait_s));
        let wait = waits::Wait {
            behind: behind(conn, &w, &row, &task_ids)?,
            due_at,
            deadline_passed: row.deadline <= now,
        };
        match waits::judge(&wait, &facts) {
            Verdict::Ends(cause) => end_wait(conn, &w, &row.kind, &row.reason, cause)?,
            Verdict::Holds { due_at: until } => {
                if row.due_at != Some(until) {
                    waits_store::set_due_at(conn, worker, &row.kind, until)?;
                }
                out.push(Wait {
                    kind: row.kind,
                    reason: row.reason,
                    started_at: row.started_at,
                    deadline: row.deadline,
                    task_ids,
                    due_at: until,
                    question_id: row.question_id,
                });
            }
        }
    }
    Ok(out)
}

/// The event kind of the ledger action `verb` names in a horch question,
/// else `None`. Only these actions close a horch question (C2).
pub fn closing_event(verb: &str) -> Option<EventKind> {
    match verb {
        "requeue" => Some(EventKind::Requeued),
        "cancel" => Some(EventKind::Cancelled),
        _ => None,
    }
}

/// Close each open horch question once a ledger action it names happened
/// after it was asked (TRF-26). The task is looked up in every run of
/// `workspace`.
fn close_acted_questions(
    conn: &Connection,
    mail: &crate::messaging::mail::Mail,
    workspace: &str,
) -> Result<()> {
    let mut runs: Option<Vec<String>> = None;
    for q in mail.open_questions()? {
        if q.closes_on.is_empty() {
            continue;
        }
        let runs = match &mut runs {
            Some(runs) => runs,
            None => {
                let mut stmt = conn.prepare("SELECT run_id FROM runs WHERE workspace_id = ?1")?;
                let ids = stmt
                    .query_map([workspace], |r| r.get(0))?
                    .collect::<rusqlite::Result<Vec<String>>>()?;
                runs.insert(ids)
            }
        };
        let asked = crate::clock::stamp(q.created_at.into());
        if acted_since(conn, runs, &q.closes_on, &asked)? {
            mail.close_question(&q.id.short())?;
        }
    }
    Ok(())
}

/// Whether an action of `actions` (`<verb> T-<n>`) has an event at or after
/// `since` for its task in one of `runs`.
fn acted_since(
    conn: &Connection,
    runs: &[String],
    actions: &[String],
    since: &str,
) -> Result<bool> {
    for action in actions {
        let Some((verb, key)) = action.split_once(' ') else {
            continue;
        };
        let Some(kind) = closing_event(verb) else {
            continue;
        };
        for run in runs {
            let Ok(task) = super::tasks::resolve(conn, run, key) else {
                continue;
            };
            let seen: i64 = conn.query_row(
                &format!(
                    "SELECT COUNT(*) FROM events WHERE task_id = ?1 AND {} AND at >= ?3",
                    util::kind_is("?2")
                ),
                params![task.task_id, kind.as_str(), since],
                |r| r.get(0),
            )?;
            if seen > 0 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Project the messaging layer's authoritative questions into ledger waits.
/// First close the horch questions whose ledger action happened. Each open
/// question of a working worker with a task gets 1 `questions` row
/// ([`questions::ask`]). The question wait of the worker points at the row
/// of its oldest open question, names the question by id (`#<id>`), and
/// keeps its cap while it points at the same row. A row whose mail
/// question is no longer open goes to `answered`, and its wait ends.
pub fn sync_questions(conn: &Connection, state: &std::path::Path, workspace: &str) -> Result<()> {
    let mail = crate::messaging::mail::Mail::under(state, workspace);
    close_acted_questions(conn, &mail, workspace)?;
    let open = mail.open_questions()?;
    close_answered_rows(conn, workspace, &open)?;
    let mut by_role: std::collections::BTreeMap<&str, Vec<&crate::messaging::mail::Message>> =
        std::collections::BTreeMap::new();
    for q in &open {
        by_role.entry(q.from.as_str()).or_default().push(q);
    }
    let working = word(WorkerStatus::Working);
    // The pass reads only the workers that a question or a question wait
    // names, not each worker of the workspace (PL-5).
    let mut asked = conn.prepare(&format!(
        "SELECT worker_id FROM workers WHERE workspace_id=?1 AND status={working} AND role=?2"
    ))?;
    for (role, asked_by_role) in &by_role {
        let ids = asked
            .query_map(params![workspace, role], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            wait_on_questions(conn, &get(conn, &id)?, asked_by_role)?;
        }
    }
    let mut waits = conn.prepare(&format!(
        "SELECT w.worker_id, w.role FROM worker_waits q JOIN workers w ON w.worker_id = q.worker_id
         WHERE q.kind='question' AND w.workspace_id=?1 AND w.status={working}"
    ))?;
    let waiting_workers = waits
        .query_map([workspace], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, role) in waiting_workers {
        if !role.is_some_and(|r| by_role.contains_key(r.as_str())) {
            end_question_wait(conn, &get(conn, &id)?)?;
        }
    }
    Ok(())
}

/// End the question wait of `w`, when it has one: no open question is
/// behind it.
fn end_question_wait(conn: &Connection, w: &Worker) -> Result<()> {
    let Some(row) = waits_store::of_worker(conn, &w.worker_id)?
        .into_iter()
        .find(|r| r.kind == QUESTION_WAIT)
    else {
        return Ok(());
    };
    end_wait(conn, w, QUESTION_WAIT, &row.reason, End::QuestionClosed)
}

/// Write the `questions` rows of the open mail questions `open` of
/// worker `w`, oldest first, and point its question wait at the oldest.
/// A worker with no task asks for nothing that a row can hold.
fn wait_on_questions(
    conn: &Connection,
    w: &Worker,
    open: &[&crate::messaging::mail::Message],
) -> Result<()> {
    let Some(task) = w.task_id.as_deref() else {
        return Ok(());
    };
    let attempt =
        crate::fleetd::task_execution::adapters::sqlite::attempts_of_worker(conn, &w.worker_id)?
            .into_iter()
            .rfind(|a| a.task_id.as_deref() == Some(task))
            .map_or_else(|| w.worker_id.clone(), |a| a.id);
    let clock = lease::fleet_clock(conn)?;
    let limit = FleetConfig::default().question_limit_s;
    let mut rows: Vec<(&crate::messaging::mail::Message, Question)> = Vec::new();
    for q in open {
        let asked_at = crate::clock::stamp(q.created_at.into());
        let ask = questions::Ask {
            task_id: task,
            attempt_id: &attempt,
            mail_id: q.id.as_str(),
            text: &q.text,
            asked_at: &asked_at,
        };
        let row = questions::ask(conn, &ask, clock, limit)?;
        // An answer recorded before its delivery closed the mail question
        // leaves the row answered: no wait points at it.
        if row.state == QuestionState::Open {
            rows.push((q, row));
        }
    }
    let Some((oldest, row)) = rows.first() else {
        return end_question_wait(conn, w);
    };
    let started: chrono::DateTime<chrono::Utc> = oldest.created_at.into();
    let earlier = waits_store::of_worker(conn, &w.worker_id)?
        .into_iter()
        .find(|r| r.kind == QUESTION_WAIT);
    // Polling never extends a wait: the cap of a wait on the same row stays.
    let due_at = earlier
        .filter(|r| r.question_id.as_deref() == Some(row.id.as_str()))
        .and_then(|r| r.due_at)
        .unwrap_or_else(|| clock.0.saturating_add(max_wait_s()));
    let wait = WaitRow {
        kind: QUESTION_WAIT.to_string(),
        reason: format!("#{} {}", oldest.id.short(), oldest.text),
        started_at: crate::clock::stamp(started),
        deadline: crate::clock::stamp(started + chrono::Duration::days(7)),
        task_ids: "[]".to_string(),
        due_at: Some(due_at),
        question_id: Some(row.id.clone()),
    };
    waits_store::put(conn, &w.worker_id, &wait)
}

/// Each `questions` row that waits for an answer, of a feature whose
/// internal run (`F-<run rowid>`, fleetd-p6 MAP.md Q5) is of `workspace`,
/// and whose mail question is not in `open`, goes to `answered`. An answer
/// through `horch tell --answers` recorded its text before the mail
/// question closed.
fn close_answered_rows(
    conn: &Connection,
    workspace: &str,
    open: &[crate::messaging::mail::Message],
) -> Result<()> {
    let mut stmt =
        conn.prepare("SELECT printf('F-%04d', rowid) FROM runs WHERE workspace_id = ?1")?;
    let features = stmt
        .query_map([workspace], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<std::collections::BTreeSet<_>>>()?;
    for (q, mail_id) in questions::open_with_mail(conn)? {
        if !features.contains(&q.feature_id) {
            continue;
        }
        if !open
            .iter()
            .any(|m| Some(m.id.as_str()) == mail_id.as_deref())
        {
            questions::answered(conn, &q, None)?;
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

    /// A run in workspace `w1` with task T-1, which the worker `worker-1`
    /// claimed.
    fn asker(store: &mut Store) -> (String, String) {
        use crate::taskledger::tasks::{self, NewTask};
        store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/r"),
                    &NewRun {
                        slug: "q".into(),
                        workspace_id: Some("w1".into()),
                        ..Default::default()
                    },
                )?;
                tasks::add(
                    tx,
                    &run,
                    &NewTask::work("t", "t.md", "opus"),
                    Actor::Orchestrator,
                )?;
                let worker = insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "opus".into(),
                        role: Some("worker-1".into()),
                        workspace_id: Some("w1".into()),
                        ..Default::default()
                    },
                )?;
                tasks::claim(tx, &worker)?;
                Ok((run.run_id, worker))
            })
            .unwrap()
    }

    fn question_wait(conn: &Connection, worker: &str) -> Option<Wait> {
        waiting(conn, worker)
            .unwrap()
            .into_iter()
            .find(|w| w.kind == "question")
    }

    /// TRF-25: the question wait names its question by id. An answer to
    /// the first question moves the wait to the second, with the second's
    /// start; an answer to the second ends it.
    #[test]
    fn trf_25_a_question_wait_is_keyed_by_question_id() {
        use crate::messaging::mail::Mail;
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = asker(&mut s);
        let mail = Mail::under(dir.path(), "w1");
        let first = mail.enqueue("worker-1", "QUESTION: which?").unwrap();
        let second = mail.enqueue("worker-1", "QUESTION: where?").unwrap();
        let open = mail.open_questions().unwrap();

        sync_questions(s.conn(), dir.path(), "w1").unwrap();
        let wait = question_wait(s.conn(), &worker).expect("a question wait");
        assert_eq!(wait.reason, format!("#{} QUESTION: which?", first.short()));

        mail.close_question(&first.short()).unwrap().unwrap();
        sync_questions(s.conn(), dir.path(), "w1").unwrap();
        let wait = question_wait(s.conn(), &worker).expect("a question wait");
        assert_eq!(wait.reason, format!("#{} QUESTION: where?", second.short()));
        assert_eq!(
            wait.started_at,
            crate::clock::stamp(open[1].created_at.into())
        );

        mail.close_question(&second.short()).unwrap().unwrap();
        sync_questions(s.conn(), dir.path(), "w1").unwrap();
        assert!(question_wait(s.conn(), &worker).is_none());
    }

    fn held_question(mail: &crate::messaging::mail::Mail, task: &str) {
        mail.enqueue(
            "horch",
            &format!(
                "[horch] QUESTION: {task} is held. Ask the operator: requeue it \
                 (horch ledger requeue {task} \"<reason>\"), or cancel it \
                 (horch ledger cancel {task} \"<reason>\")."
            ),
        )
        .unwrap();
    }

    fn hold(conn: &Connection, run: &str) -> crate::taskledger::tasks::Task {
        conn.execute(
            "UPDATE tasks SET status = 'escalated', held_reason = '3 attempts failed'
             WHERE run_id = ?1",
            [run],
        )
        .unwrap();
        crate::taskledger::tasks::resolve(conn, run, "T-1").unwrap()
    }

    /// TRF-26: a horch question stays open until the ledger action it
    /// names runs: `requeue T-1` closes it.
    #[test]
    fn trf_26_a_requeue_closes_the_held_question() {
        use crate::messaging::mail::Mail;
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (run, _) = asker(&mut s);
        let task = hold(s.conn(), &run);
        let mail = Mail::under(dir.path(), "w1");
        held_question(&mail, "T-1");
        held_question(&mail, "T-9");

        sync_questions(s.conn(), dir.path(), "w1").unwrap();
        assert_eq!(mail.open_questions().unwrap().len(), 2);

        crate::taskledger::tasks::requeue_by_operator(s.conn(), &task, "go").unwrap();
        sync_questions(s.conn(), dir.path(), "w1").unwrap();
        let open = mail.open_questions().unwrap();
        assert_eq!(open.len(), 1, "only the T-1 question closes");
        assert!(open[0].text.contains("T-9 is held"));
    }

    /// TRF-26: `cancel T-1`, the other action the question names, closes
    /// it too.
    #[test]
    fn trf_26_a_cancel_closes_the_held_question() {
        use crate::messaging::mail::Mail;
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (run, _) = asker(&mut s);
        let task = hold(s.conn(), &run);
        let mail = Mail::under(dir.path(), "w1");
        held_question(&mail, "T-1");

        crate::taskledger::tasks::cancel(s.conn(), &task, "drop").unwrap();
        sync_questions(s.conn(), dir.path(), "w1").unwrap();
        assert!(mail.open_questions().unwrap().is_empty());
    }

    #[test]
    fn trf_26_only_requeue_and_cancel_close_a_question() {
        assert_eq!(closing_event("requeue"), Some(EventKind::Requeued));
        assert_eq!(closing_event("cancel"), Some(EventKind::Cancelled));
        assert_eq!(closing_event("status"), None);
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

    /// The refusal events and the open needs-you items of worker `id`.
    fn refusals(conn: &Connection, id: &str) -> (i64, i64) {
        let count = |sql: &str| -> i64 { conn.query_row(sql, [id], |r| r.get(0)).unwrap() };
        (
            count(
                "SELECT count(*) FROM events WHERE kind = 'transition' AND entity = 'worker'
                 AND entity_id = ?1 AND to_state = 'starting' AND cause LIKE 'illegal_transition:%'",
            ),
            count(
                "SELECT count(*) FROM needs_you WHERE entity = 'worker' AND entity_id = ?1
                 AND state = 'open'",
            ),
        )
    }

    /// Review F1: a refused move rolls back the whole command, and the
    /// opener of the transaction then records the refusal event and the
    /// needs-you item.
    #[test]
    fn a_refused_move_rolls_back_the_command_and_stays_on_record() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let id = author(&mut s);
        supervisor_started(s.conn(), &id, 4242, Some(1)).unwrap();
        agent_started(s.conn(), &id, 4243, Some(2)).unwrap();
        let before = get(s.conn(), &id).unwrap().last_checkin_at;

        // Store::immediate opens the transaction: claiming -> starting is
        // refused after an earlier write of the same command.
        let err = s
            .immediate(|tx| {
                tx.execute(
                    "UPDATE workers SET last_checkin_at = '2000-01-01T00:00:00Z'
                     WHERE worker_id = ?1",
                    [&id],
                )?;
                supervisor_started(tx, &id, 5000, Some(3))
            })
            .unwrap_err();
        assert!(err.to_string().contains("claiming -> starting"), "{err:#}");
        let w = get(s.conn(), &id).unwrap();
        assert_eq!(w.status().unwrap(), WorkerStatus::Claiming);
        assert_eq!(w.last_checkin_at, before, "the command rolled back");
        assert_eq!(w.supervisor_pid, Some(4242));
        assert_eq!(refusals(s.conn(), &id), (1, 1));

        // util::atomic opens the transaction for a caller in autocommit mode.
        assert!(supervisor_started(s.conn(), &id, 5000, Some(3)).is_err());
        assert_eq!(get(s.conn(), &id).unwrap().supervisor_pid, Some(4242));
        assert_eq!(
            refusals(s.conn(), &id),
            (2, 1),
            "1 needs-you item for each row"
        );
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

    /// The `fleetd` row at fleet clock `clock`, or the clock moved there.
    fn set_clock(conn: &Connection, clock: i64) {
        conn.execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, 1, 1, 'then', ?1, 'then')
             ON CONFLICT (id) DO UPDATE SET fleet_clock_s = excluded.fleet_clock_s",
            [clock],
        )
        .unwrap();
    }

    /// The `wait_ended` event texts of `worker`.
    fn ended(conn: &Connection, worker: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT text FROM events WHERE kind = 'wait_ended' AND worker_id = ?1
                 ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([worker], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn row(conn: &Connection, worker: &str, kind: &str) -> Option<WaitRow> {
        waits_store::of_worker(conn, worker)
            .unwrap()
            .into_iter()
            .find(|r| r.kind == kind)
    }

    /// A claimed worker whose agent is gone, at fleet clock 1000.
    fn claimed(s: &mut Store) -> (String, String) {
        let (run, worker) = asker(s);
        set_clock(s.conn(), 1_000);
        (run, worker)
    }

    #[test]
    fn wait_gets_due_at_max_wait_s_on_the_fleet_clock() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = claimed(&mut s);
        set_wait(s.conn(), &worker, BACKGROUND_WAIT, "build", &[], 24 * 3600).unwrap();
        assert_eq!(
            row(s.conn(), &worker, BACKGROUND_WAIT).unwrap().due_at,
            Some(4_600)
        );
        let waits = waiting(s.conn(), &worker).unwrap();
        assert_eq!(waits.len(), 1, "{waits:?}");
        assert_eq!(waits[0].due_at, 4_600);
    }

    #[test]
    fn wait_past_due_at_ends_with_an_event() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = claimed(&mut s);
        set_wait(s.conn(), &worker, BACKGROUND_WAIT, "build", &[], 24 * 3600).unwrap();
        set_clock(s.conn(), 4_599);
        assert_eq!(waiting(s.conn(), &worker).unwrap().len(), 1);
        set_clock(s.conn(), 4_600);
        assert!(waiting(s.conn(), &worker).unwrap().is_empty());
        assert!(row(s.conn(), &worker, BACKGROUND_WAIT).is_none());
        assert_eq!(
            ended(s.conn(), &worker),
            ["max_wait_reached: the background wait ended (build)"]
        );
    }

    #[test]
    fn background_wait_of_a_worker_with_no_agent_pid_holds_to_its_cap() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = claimed(&mut s);
        let w = get(s.conn(), &worker).unwrap();
        assert_eq!(w.agent_pid, None, "the worker records no agent pid");
        assert_eq!(agent_child(&w), Child::Unknown);
        set_wait(s.conn(), &worker, BACKGROUND_WAIT, "build", &[], 24 * 3600).unwrap();
        set_clock(s.conn(), 4_599);
        assert_eq!(waiting(s.conn(), &worker).unwrap().len(), 1, "it holds");
        assert!(ended(s.conn(), &worker).is_empty());
        set_clock(s.conn(), 4_600);
        assert!(waiting(s.conn(), &worker).unwrap().is_empty());
        assert_eq!(
            ended(s.conn(), &worker),
            ["max_wait_reached: the background wait ended (build)"]
        );
    }

    #[test]
    fn background_wait_is_capped_at_max_wait_s() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = claimed(&mut s);
        // `horch ledger progress "waiting: ..."` sets a calendar deadline
        // of 24 h; the fleet clock caps it at max_wait_s.
        progress(
            s.conn(),
            &worker,
            ProgressSource::Agent,
            EventKind::Progress,
            "waiting: a long test",
        )
        .unwrap();
        let wait = row(s.conn(), &worker, BACKGROUND_WAIT).unwrap();
        assert_eq!(wait.due_at, Some(4_600));
        set_clock(s.conn(), 4_600);
        assert!(waiting(s.conn(), &worker).unwrap().is_empty());
        assert_eq!(ended(s.conn(), &worker).len(), 1);
    }

    fn mail_question(dir: &Path, text: &str) -> crate::messaging::mail::Id {
        crate::messaging::mail::Mail::under(dir, "w1")
            .enqueue("worker-1", text)
            .unwrap()
    }

    #[test]
    fn question_wait_points_at_its_open_questions_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = claimed(&mut s);
        mail_question(dir.path(), "QUESTION: which?");
        mail_question(dir.path(), "QUESTION: where?");
        sync_questions(s.conn(), dir.path(), "w1").unwrap();
        sync_questions(s.conn(), dir.path(), "w1").unwrap();

        let rows: Vec<(String, String, String, i64)> = {
            let mut stmt = s
                .conn()
                .prepare("SELECT id, feature_id, state, due_at FROM questions ORDER BY id")
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        assert_eq!(rows.len(), 2, "1 row for each open question: {rows:?}");
        assert!(
            rows.iter()
                .all(|r| r.1 == "F-0001" && r.2 == "open" && r.3 == 1_900),
            "the feature of the run of the task: {rows:?}"
        );
        let wait = question_wait(s.conn(), &worker).expect("a question wait");
        assert_eq!(wait.question_id.as_deref(), Some(rows[0].0.as_str()));
        assert!(rows[0].0.ends_with("/Q1"), "{rows:?}");
        assert_eq!(wait.due_at, 4_600);
    }

    #[test]
    fn question_wait_ends_when_its_row_is_answered() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = claimed(&mut s);
        let id = mail_question(dir.path(), "QUESTION: which?");
        sync_questions(s.conn(), dir.path(), "w1").unwrap();
        let qid = question_wait(s.conn(), &worker)
            .unwrap()
            .question_id
            .unwrap();

        // The answer by id records its text, then closes the mail question.
        let answered = questions::answer_mail(s.conn(), id.as_str(), "the first").unwrap();
        assert_eq!(answered.as_deref(), Some(qid.as_str()));
        assert!(question_wait(s.conn(), &worker).is_none());
        assert_eq!(
            ended(s.conn(), &worker),
            ["question_closed: the question wait ended (#".to_string()
                + &id.short()
                + " QUESTION: which?)"]
        );
    }

    #[test]
    fn reviewers_wait_with_no_live_review_ends() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (run, worker) = claimed(&mut s);
        let review = review_of(&mut s, &run, &worker);
        set_wait(
            s.conn(),
            &worker,
            REVIEWS_WAIT,
            "reviews",
            std::slice::from_ref(&review),
            7 * 24 * 3600,
        )
        .unwrap();
        set_clock(s.conn(), 20_000);
        let waits = waiting(s.conn(), &worker).unwrap();
        assert_eq!(waits.len(), 1, "a live review moves the cap: {waits:?}");
        assert_eq!(waits[0].due_at, 23_600);

        s.conn()
            .execute(
                "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                [&review],
            )
            .unwrap();
        assert!(waiting(s.conn(), &worker).unwrap().is_empty());
        assert_eq!(
            ended(s.conn(), &worker),
            ["reviews_done: the reviewers wait ended (reviews)"]
        );
    }

    /// A review task of the task of `worker`, in `ready`.
    fn review_of(s: &mut Store, run: &str, worker: &str) -> String {
        use crate::taskledger::tasks::{self, NewTask};
        let parent = get(s.conn(), worker).unwrap().task_id.unwrap();
        s.immediate(|tx| {
            let run = runs::get(tx, run)?;
            let mut new = NewTask::work("review", "r.md", "codex-reviewer");
            new.kind = crate::taskledger::model::TaskKind::Review;
            new.parent_task_id = Some(parent.clone());
            Ok(tasks::add(tx, &run, &new, Actor::Orchestrator)?.task_id)
        })
        .unwrap()
    }

    /// The detached finish wait of `worker`: deadline 4 h from now.
    fn finish_wait(s: &Store, worker: &str) {
        set_wait(
            s.conn(),
            worker,
            crate::taskledger::finish::detach::FINISH_WAIT,
            "horch ledger finish runs detached",
            &[],
            4 * 3600,
        )
        .unwrap();
    }

    /// Orchestrator rule of T-267: a finish of 3 h whose process lives
    /// keeps its wait. The agent leads a process group with 1 child.
    #[cfg(unix)]
    #[test]
    fn finish_wait_of_3_h_with_a_live_process_is_not_ended() {
        use std::os::unix::process::CommandExt;
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = claimed(&mut s);
        let mut agent = std::process::Command::new("sh")
            .args(["-c", "sleep 60 & wait"])
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = agent.id();
        let child_up = (0..100).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            horch_platform::process::group_members(pid).is_ok_and(|m| m.iter().any(|p| *p != pid))
        });
        assert!(child_up, "the agent has a child");
        s.conn()
            .execute(
                "UPDATE workers SET agent_pid = ?2, agent_started = ?3 WHERE worker_id = ?1",
                params![
                    worker,
                    pid,
                    crate::procid::start_time(pid).map(|t| t as i64)
                ],
            )
            .unwrap();
        finish_wait(&s, &worker);
        for clock in [3_000, 6_000, 9_000, 1_000 + 3 * 3600] {
            set_clock(s.conn(), clock);
            let waits = waiting(s.conn(), &worker).unwrap();
            assert_eq!(waits.len(), 1, "fleet clock {clock}: {waits:?}");
            assert_eq!(waits[0].due_at, clock + 3_600);
        }
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .status();
        let _ = agent.wait();
        assert!(ended(s.conn(), &worker).is_empty());
    }

    #[test]
    fn finish_wait_with_open_review_tasks_is_not_ended() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (run, worker) = claimed(&mut s);
        review_of(&mut s, &run, &worker);
        finish_wait(&s, &worker);
        set_clock(s.conn(), 1_000 + 3 * 3600);
        let waits = waiting(s.conn(), &worker).unwrap();
        assert_eq!(waits.len(), 1, "{waits:?}");
        assert_eq!(waits[0].due_at, 1_000 + 4 * 3600);
    }

    #[test]
    fn background_wait_whose_process_is_gone_ends_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = claimed(&mut s);
        // The recorded agent is a process that has ended.
        let mut gone = std::process::Command::new("true").spawn().unwrap();
        let pid = gone.id();
        gone.wait().unwrap();
        s.conn()
            .execute(
                "UPDATE workers SET agent_pid = ?2 WHERE worker_id = ?1",
                params![worker, pid],
            )
            .unwrap();
        finish_wait(&s, &worker);
        // No step runs and no review is live: no proof holds the wait
        // (MAP.md Q23, option 1).
        assert!(waiting(s.conn(), &worker).unwrap().is_empty());
        assert_eq!(
            ended(s.conn(), &worker),
            ["child_gone: the background wait ended (horch ledger finish runs detached)"]
        );
    }

    /// A finish step that stays `running` after its process died proves
    /// work. The calendar deadline of the finish wait (3 x the check
    /// timeout + 1 h) ends it, so it never holds for ever.
    #[test]
    fn a_stale_running_step_holds_the_wait_only_to_its_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = claimed(&mut s);
        let task = get(s.conn(), &worker).unwrap().task_id.unwrap();
        s.conn()
            .execute(
                "INSERT INTO task_steps (task_id, worker_id, seq, name, status)
                 VALUES (?1, ?2, 1, 'check', 'running')",
                params![task, worker],
            )
            .unwrap();
        finish_wait(&s, &worker);
        set_clock(s.conn(), 1_000 + 10 * 3600);
        assert_eq!(
            waiting(s.conn(), &worker).unwrap().len(),
            1,
            "the step proves work"
        );
        s.conn()
            .execute(
                "UPDATE worker_waits SET deadline = '2000-01-01T00:00:00Z' WHERE worker_id = ?1",
                [&worker],
            )
            .unwrap();
        assert!(waiting(s.conn(), &worker).unwrap().is_empty());
        assert_eq!(
            ended(s.conn(), &worker),
            ["deadline_passed: the background wait ended (horch ledger finish runs detached)"]
        );
    }
}
