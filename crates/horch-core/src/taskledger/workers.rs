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
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::questions_store;
use crate::fleetd::ideas::domain::questions::State as QuestionState;
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::adapters::task_files::attempt_dir;
use crate::fleetd::task_execution::adapters::waits_store::{self, WaitRow};
use crate::fleetd::task_execution::application::lease;
use crate::fleetd::task_execution::domain::waits::{
    self, Behind, Child, End, Facts, Proof, Verdict,
};
use crate::fleetd::worker_profiles::domain::profile::{Liveness, Mode};
use crate::fleetd::worktree_custody::adapters::executors::fleet_root;

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
    pub handoff_path: Option<String>,
    pub target_task_id: Option<String>,
    pub backend: crate::runtime::Backend,
}

impl Worker {
    pub fn status(&self) -> Result<WorkerStatus> {
        WorkerStatus::parse(&self.status)
    }

    pub fn label(&self) -> &str {
        self.role.as_deref().unwrap_or(&self.worker_id)
    }

    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Worker> {
        let backend_raw: String = r.get("backend")?;
        let backend = backend_raw
            .parse::<crate::runtime::Backend>()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
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
            backend,
        })
    }
}

pub fn get(conn: &Connection, worker_id: &str) -> Result<Worker> {
    find(conn, worker_id)?.with_context(|| format!("no worker {worker_id} in the task ledger"))
}

pub fn find(conn: &Connection, worker_id: &str) -> Result<Option<Worker>> {
    optional(conn.query_row(
        "SELECT * FROM workers WHERE worker_id = ?1",
        [worker_id],
        Worker::from_row,
    ))
}

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

pub fn of_run(conn: &Connection, run_id: &str) -> Result<Vec<Worker>> {
    let mut stmt =
        conn.prepare("SELECT * FROM workers WHERE run_id = ?1 ORDER BY spawned_at, rowid")?;
    let rows = stmt
        .query_map([run_id], Worker::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn attempts_at(conn: &Connection, task_id: &str) -> Result<Vec<Worker>> {
    let mut stmt = conn
        .prepare("SELECT * FROM workers WHERE task_id = ?1 ORDER BY attempt, claimed_at, rowid")?;
    let rows = stmt
        .query_map([task_id], Worker::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

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
    pub target_task_id: Option<String>,
    pub backend: crate::runtime::Backend,
}

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
            legacy_record_id, status, origin, spawned_at, target_task_id, backend)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
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
            new.backend.as_str(),
        ],
    )?;
    Ok(id)
}

pub fn set_pane(conn: &Connection, worker_id: &str, pane_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE workers SET pane_id = ?2 WHERE worker_id = ?1",
        params![worker_id, pane_id],
    )?;
    Ok(())
}

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

pub fn checkin(conn: &Connection, worker_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE workers SET last_checkin_at = ?2 WHERE worker_id = ?1",
        params![worker_id, util::now()],
    )?;
    Ok(())
}

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

fn is_live(conn: &Connection, worker_id: &str) -> Result<bool> {
    Ok(find(conn, worker_id)?
        .map(|w| w.status())
        .transpose()?
        .is_some_and(|s| sql_words::WORKERS_LIVE.contains(&s)))
}

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

pub const LAUNCH_GRACE_S: i64 = 120;

pub const CLAIM_GRACE_S: i64 = 600;

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

pub fn supervisor_alive(w: &Worker) -> bool {
    w.supervisor_pid
        .is_some_and(|p| crate::procid::alive(p as u32, w.supervisor_started.map(|s| s as u64)))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Preclaim {
    pub worker: Worker,
    pub ended: Option<String>,
    pub note: String,
}

const NO_CLAIM: &str = "No claim:";

pub fn sweep_preclaim(
    conn: &Connection,
    run: &super::runs::Run,
    alive: &dyn Fn(&Worker) -> bool,
) -> Result<Vec<Preclaim>> {
    sweep_preclaim_of(conn, run, of_run(conn, &run.run_id)?, alive)
}

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

pub fn set_exit_code(conn: &Connection, worker_id: &str, exit_code: Option<i32>) -> Result<()> {
    conn.execute(
        "UPDATE workers SET exit_code = COALESCE(?2, exit_code), ended_at = COALESCE(ended_at, ?3)
         WHERE worker_id = ?1",
        params![worker_id, exit_code, util::now()],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Wait {
    pub kind: String,
    pub reason: String,
    pub started_at: String,
    pub deadline: String,
    pub task_ids: Vec<String>,
    pub due_at: i64,
    pub question_id: Option<String>,
}

pub const REVIEWS_WAIT: &str = "reviewers";

pub const QUESTION_WAIT: &str = "question";

pub const BACKGROUND_WAIT: &str = "background";

pub const REVIEWS_WAIT_SECS: i64 = 7 * 24 * 3600;

fn max_wait_s() -> i64 {
    i64::try_from(Liveness::default().max_wait_s).unwrap_or(i64::MAX)
}

pub fn holds_for_finish(wait: &Wait) -> bool {
    wait.kind == REVIEWS_WAIT && wait.task_ids.is_empty()
}

pub fn hold_for_finish(conn: &Connection, worker: &str) -> Result<()> {
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

const REVIEW_WORKING: [TaskStatus; 3] =
    [TaskStatus::Ready, TaskStatus::Leased, TaskStatus::Running];

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

fn behind(conn: &Connection, w: &Worker, row: &WaitRow, task_ids: &[String]) -> Result<Behind> {
    Ok(match row.kind.as_str() {
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

fn judged(conn: &Connection, worker: &str) -> Result<Vec<(WaitRow, Vec<String>, Verdict)>> {
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
        let verdict = waits::judge(&wait, &facts);
        out.push((row, task_ids, verdict));
    }
    Ok(out)
}

fn held(row: WaitRow, task_ids: Vec<String>, until: i64) -> Wait {
    Wait {
        kind: row.kind,
        reason: row.reason,
        started_at: row.started_at,
        deadline: row.deadline,
        task_ids,
        due_at: until,
        question_id: row.question_id,
    }
}

pub fn waits_now(conn: &Connection, worker: &str) -> Result<Vec<Wait>> {
    Ok(judged(conn, worker)?
        .into_iter()
        .filter_map(|(row, task_ids, verdict)| match verdict {
            Verdict::Ends(_) => None,
            Verdict::Holds { due_at } => Some(held(row, task_ids, due_at)),
        })
        .collect())
}

pub fn waiting(conn: &Connection, worker: &str) -> Result<Vec<Wait>> {
    let verdicts = judged(conn, worker)?;
    if verdicts.is_empty() {
        return Ok(Vec::new());
    }
    let w = get(conn, worker)?;
    let mut out = Vec::new();
    for (row, task_ids, verdict) in verdicts {
        match verdict {
            Verdict::Ends(cause) => end_wait(conn, &w, &row.kind, &row.reason, cause)?,
            Verdict::Holds { due_at: until } => {
                if row.due_at != Some(until) {
                    waits_store::set_due_at(conn, worker, &row.kind, until)?;
                }
                out.push(held(row, task_ids, until));
            }
        }
    }
    Ok(out)
}

pub fn closing_event(verb: &str) -> Option<EventKind> {
    match verb {
        "requeue" => Some(EventKind::Requeued),
        "cancel" => Some(EventKind::Cancelled),
        _ => None,
    }
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
    let waits = waits_now(conn, &w.worker_id)?;
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
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
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
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                let mut t = NewTask::work("t", "t.md", "opus");
                t.worktree_mode = crate::taskledger::model::WorktreeMode::Shared;
                let task = tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id;
                let new = NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "opus".into(),
                    role: Some("worker-1".into()),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                };
                let worker =
                    crate::taskledger::testing::bind_live(tx, &task, &new, None)?.worker_id;
                Ok((run.run_id, worker))
            })
            .unwrap()
    }

    #[test]
    fn trf_26_only_requeue_and_cancel_close_a_question() {
        assert_eq!(closing_event("requeue"), Some(EventKind::Requeued));
        assert_eq!(closing_event("cancel"), Some(EventKind::Cancelled));
        assert_eq!(closing_event("status"), None);
    }

    #[test]
    fn agent_started_refuses_an_unknown_worker() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let err = agent_started(s.conn(), "w-missing", 4243, Some(2)).unwrap_err();
        assert_eq!(err.to_string(), "no worker w-missing in the task ledger");
    }

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

    #[test]
    fn a_refused_move_rolls_back_the_command_and_stays_on_record() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let id = author(&mut s);
        supervisor_started(s.conn(), &id, 4242, Some(1)).unwrap();
        agent_started(s.conn(), &id, 4243, Some(2)).unwrap();
        let before = get(s.conn(), &id).unwrap().last_checkin_at;

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

    fn set_clock(conn: &Connection, clock: i64) {
        conn.execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, 1, 1, 'then', ?1, 'then')
             ON CONFLICT (id) DO UPDATE SET fleet_clock_s = excluded.fleet_clock_s",
            [clock],
        )
        .unwrap();
    }

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
    fn waits_now_leaves_out_an_ended_wait_with_no_write() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (_, worker) = claimed(&mut s);
        set_wait(s.conn(), &worker, BACKGROUND_WAIT, "build", &[], 24 * 3600).unwrap();
        set_clock(s.conn(), 4_600);
        assert!(waits_now(s.conn(), &worker).unwrap().is_empty());
        assert!(row(s.conn(), &worker, BACKGROUND_WAIT).is_some());
        assert!(ended(s.conn(), &worker).is_empty());
        assert!(waiting(s.conn(), &worker).unwrap().is_empty());
        assert_eq!(ended(s.conn(), &worker).len(), 1);
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
        assert!(waiting(s.conn(), &worker).unwrap().is_empty());
        assert_eq!(
            ended(s.conn(), &worker),
            ["child_gone: the background wait ended (horch ledger finish runs detached)"]
        );
    }

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
