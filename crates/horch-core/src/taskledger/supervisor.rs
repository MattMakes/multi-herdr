//! What `horch worker` (the supervisor) writes to the task ledger
//! (section 4.6, #34): its own PID, its agent's PID, a checkin every
//! `heartbeat_every_s`, the session and transcript once known, and the end.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use rusqlite::params;

use super::model::{Actor, EventKind, StopRequester, StopResult, StopTarget, WorkerStatus};
use super::store::Store;
use super::tasks;
use super::util::{self, About};
use super::workers;

/// Record that the supervisor runs: this process's PID and start time, and
/// the role spawn gave it.
pub fn started(db: &Path, worker_id: &str, role: &str) -> Result<()> {
    let pid = std::process::id();
    let mut s = Store::open(db)?;
    s.immediate(|tx| {
        workers::supervisor_started(tx, worker_id, pid, crate::procid::start_time(pid))?;
        tx.execute(
            "UPDATE workers SET role = COALESCE(role, ?2) WHERE worker_id = ?1",
            params![worker_id, role],
        )?;
        Ok(())
    })
}

/// Record the agent child's PID and start time.
pub fn agent_started(db: &Path, worker_id: &str, pid: u32) -> Result<()> {
    let mut s = Store::open(db)?;
    s.immediate(|tx| workers::agent_started(tx, worker_id, pid, crate::procid::start_time(pid)))
}

/// Looks up the session id and transcript path while the agent runs.
pub type SessionLookup = Box<dyn Fn() -> (Option<String>, Option<String>) + Send>;

/// The checkin thread. Dropping it, or [`Checkins::stop`], ends it within 1
/// tick.
pub struct Checkins {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Checkins {
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Checkins {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The checkin interval of the worker's run.
pub fn interval(db: &Path, worker_id: &str) -> Duration {
    let secs = Store::open(db)
        .ok()
        .and_then(|s| {
            s.conn()
                .query_row(
                    "SELECT r.heartbeat_every_s FROM workers w JOIN runs r ON r.run_id = w.run_id
                     WHERE w.worker_id = ?1",
                    [worker_id],
                    |r| r.get::<_, i64>(0),
                )
                .ok()
        })
        .unwrap_or(30)
        .max(1);
    Duration::from_secs(secs as u64)
}

/// Write `last_checkin_at = now` every `every` until stopped, and fill the
/// session id and transcript path as soon as `lookup` knows them.
pub fn start_checkins(
    db: PathBuf,
    worker_id: String,
    every: Duration,
    lookup: SessionLookup,
) -> Checkins {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::spawn(move || {
        let tick = Duration::from_millis(100).min(every);
        let mut since = Duration::ZERO;
        let mut session_known = false;
        let mut log = CheckinLog::default();
        loop {
            if flag.load(Ordering::SeqCst) {
                return;
            }
            if since >= every {
                since = Duration::ZERO;
                let result = beat(&db, &worker_id, &lookup, &mut session_known)
                    .map_err(|e| format!("{e:#}"));
                if let Some(line) = log.record(result, Instant::now()) {
                    eprintln!("{line}");
                }
            }
            std::thread::sleep(tick);
            since += tick;
        }
    });
    Checkins {
        stop,
        thread: Some(thread),
    }
}

/// The longest time 1 repeated checkin error stays silent.
pub const REPEAT_EVERY: Duration = Duration::from_secs(10 * 60);

/// What the checkin thread printed, so 1 error that repeats every checkin
/// does not fill the pane (TRF-49).
#[derive(Debug, Default)]
pub struct CheckinLog {
    /// The text of the last failure, while checkins fail.
    last_error: Option<String>,
    /// How many checkins in a row failed with `last_error`.
    repeats: u64,
    /// How many checkins in a row failed, with any text.
    failures: u64,
    /// When the last failure line was printed.
    printed_at: Option<Instant>,
}

impl CheckinLog {
    /// The line to print for 1 checkin `result` at `now`, if any. A failure
    /// prints the first time and when its text changes. The same failure
    /// prints again only after [`REPEAT_EVERY`], with its repeat count. The
    /// first success after failures prints 1 recovery line.
    pub fn record(&mut self, result: Result<(), String>, now: Instant) -> Option<String> {
        let text = match result {
            Ok(()) => {
                let failures = std::mem::take(self).failures;
                return (failures > 0)
                    .then(|| format!("horch worker: checkin recovered after {failures} failures"));
            }
            Err(text) => text,
        };
        self.failures += 1;
        if self.last_error.as_deref() != Some(text.as_str()) {
            self.last_error = Some(text.clone());
            self.repeats = 1;
            self.printed_at = Some(now);
            return Some(format!("horch worker: checkin failed: {text}"));
        }
        self.repeats += 1;
        let due = self
            .printed_at
            .is_none_or(|at| now.saturating_duration_since(at) >= REPEAT_EVERY);
        if !due {
            return None;
        }
        self.printed_at = Some(now);
        Some(format!(
            "horch worker: checkin failed {n} times in a row: {text}",
            n = self.repeats
        ))
    }
}

fn beat(
    db: &Path,
    worker_id: &str,
    lookup: &SessionLookup,
    session_known: &mut bool,
) -> Result<()> {
    let s = Store::open(db)?;
    workers::checkin(s.conn(), worker_id)?;
    if !*session_known {
        let (session, transcript) = lookup();
        if session.is_some() || transcript.is_some() {
            workers::set_session(
                s.conn(),
                worker_id,
                session.as_deref(),
                transcript.as_deref(),
            )?;
            let w = workers::get(s.conn(), worker_id)?;
            *session_known = w.session_id.is_some() && w.transcript_path.is_some();
        }
    }
    Ok(())
}

/// How a worker's agent ended, for the supervisor's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// It ran `horch ledger done`, or held no task.
    Clean,
    /// It held a task without finishing it: the task is queued again.
    Requeued { task: String, persona: String },
    /// It held a task that has now used up its attempts: the task is held
    /// for the operator (#168).
    Held { task: String },
}

/// Settle the worker row after the agent exited with `code`.
pub fn agent_exited(db: &Path, worker_id: &str, code: Option<i32>) -> Result<Ended> {
    let mut s = Store::open(db)?;
    s.immediate(|tx| settle(tx, worker_id, code))
}

fn settle(tx: &rusqlite::Connection, worker_id: &str, code: Option<i32>) -> Result<Ended> {
    let w = workers::get(tx, worker_id)?;
    let status = w.status()?;
    let about = About {
        run: w.run_id.as_deref(),
        task: w.task_id.as_deref(),
        worker: Some(worker_id),
    };
    let reason = record_exit(tx, worker_id, status, code)?;
    util::event(tx, &about, Actor::Supervisor, EventKind::Exited, &reason)?;
    if status == WorkerStatus::Working {
        return requeue_after_exit(tx, &w, &reason);
    }
    Ok(Ended::Clean)
}

/// End the worker per [`exit_rule`], or only record the exit code. Returns
/// the reason text of the exit event.
fn record_exit(
    tx: &rusqlite::Connection,
    worker_id: &str,
    status: WorkerStatus,
    code: Option<i32>,
) -> Result<String> {
    let how = exit_text(code);
    let Some((end, suffix)) = exit_rule(status) else {
        workers::set_exit_code(tx, worker_id, code)?;
        return Ok(how);
    };
    let reason = format!("{how}{suffix}");
    workers::end(tx, worker_id, end, &reason, code)?;
    Ok(reason)
}

/// How the agent ended, as the exit event words it.
fn exit_text(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("agent exited with code {c}"),
        None => "agent was ended by a signal".to_string(),
    }
}

/// The end status and reason suffix of a worker whose agent exited in
/// `status`; `None` keeps the row and only records the exit code.
fn exit_rule(status: WorkerStatus) -> Option<(WorkerStatus, &'static str)> {
    match status {
        WorkerStatus::Working => Some((WorkerStatus::Incomplete, " before horch ledger done")),
        WorkerStatus::Starting | WorkerStatus::Spawning => Some((WorkerStatus::LaunchFailed, "")),
        WorkerStatus::Claiming => Some((WorkerStatus::NoTask, " before it claimed a task")),
        _ => None,
    }
}

/// Queue the task of a worker that exited while working, or hold it.
fn requeue_after_exit(
    tx: &rusqlite::Connection,
    w: &workers::Worker,
    reason: &str,
) -> Result<Ended> {
    let task = w.task_id.clone().unwrap_or_default();
    let how = tasks::requeue(tx, &task, &format!("{} {reason}", w.label()))?;
    let t = tasks::get(tx, &task)?;
    Ok(match how {
        tasks::Requeue::Held => Ended::Held { task: t.short() },
        _ => Ended::Requeued {
            task: t.short(),
            persona: t.claimable_by.unwrap_or_default(),
        },
    })
}

/// Record that the worker closed its own pane.
pub fn closed_own_pane(db: &Path, worker_id: &str, pane: &str, outcome: &Result<()>) -> Result<()> {
    let s = Store::open(db)?;
    s.conn().execute(
        "INSERT INTO stops (worker_id, requested_by, reason, target, pane_id, result, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            worker_id,
            StopRequester::SelfExit.as_str(),
            match outcome {
                Ok(()) => "the worker finished and closed its own pane".to_string(),
                Err(e) => format!("closing its own pane failed: {e:#}"),
            },
            StopTarget::Pane.as_str(),
            pane,
            match outcome {
                Ok(()) => StopResult::PaneClosed.as_str(),
                Err(_) => StopResult::Error.as_str(),
            },
            util::now()
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::workers::NewWorker;

    struct Fixture {
        _dir: tempfile::TempDir,
        db: PathBuf,
        store: Store,
        worker: String,
    }

    /// A worker in `status` (`working` holds a claimed task).
    fn fixture(status: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("l.sqlite3");
        let mut store = Store::open(&db).unwrap();
        let worker = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/r"),
                    &NewRun {
                        slug: "s".into(),
                        ..Default::default()
                    },
                )?;
                let id = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "opus".into(),
                        role: Some("opus-x".into()),
                        ..Default::default()
                    },
                )?;
                if status == "working" {
                    tasks::add(
                        tx,
                        &run,
                        &NewTask::work("a", "tickets/a.md", "opus"),
                        Actor::Orchestrator,
                    )?;
                    tasks::claim(tx, &id)?;
                }
                tx.execute(
                    "UPDATE workers SET status = ?2 WHERE worker_id = ?1 AND status != 'working'",
                    params![id, status],
                )?;
                Ok(id)
            })
            .unwrap();
        Fixture {
            _dir: dir,
            db,
            store,
            worker,
        }
    }

    fn row(f: &Fixture) -> (String, Option<String>, Option<i64>) {
        f.store
            .conn()
            .query_row(
                "SELECT status, end_reason, exit_code FROM workers WHERE worker_id = ?1",
                [&f.worker],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    }

    fn exited_text(f: &Fixture) -> Vec<String> {
        let mut q = f
            .store
            .conn()
            .prepare("SELECT text FROM events WHERE worker_id = ?1 AND kind = 'exited'")
            .unwrap();
        let rows = q.query_map([&f.worker], |r| r.get(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    #[test]
    fn a_working_agent_that_exits_ends_incomplete_and_requeues_its_task() {
        let f = fixture("working");
        let ended = agent_exited(&f.db, &f.worker, Some(3)).unwrap();
        assert_eq!(
            ended,
            Ended::Requeued {
                task: "T-1".into(),
                persona: "opus".into()
            }
        );
        let reason = "agent exited with code 3 before horch ledger done";
        assert_eq!(row(&f), ("incomplete".into(), Some(reason.into()), Some(3)));
        assert_eq!(exited_text(&f), [reason]);
        let status: String = f
            .store
            .conn()
            .query_row("SELECT status FROM tasks WHERE seq = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "queued");
    }

    #[test]
    fn a_starting_or_spawning_agent_that_exits_is_a_failed_launch() {
        for status in ["starting", "spawning"] {
            let f = fixture(status);
            assert_eq!(agent_exited(&f.db, &f.worker, None).unwrap(), Ended::Clean);
            let how = "agent was ended by a signal";
            assert_eq!(row(&f), ("launch_failed".into(), Some(how.into()), None));
            assert_eq!(exited_text(&f), [how]);
        }
    }

    #[test]
    fn a_claiming_agent_that_exits_ends_with_no_task() {
        let f = fixture("claiming");
        assert_eq!(
            agent_exited(&f.db, &f.worker, Some(0)).unwrap(),
            Ended::Clean
        );
        let reason = "agent exited with code 0 before it claimed a task";
        assert_eq!(row(&f), ("no_task".into(), Some(reason.into()), Some(0)));
        assert_eq!(exited_text(&f), [reason]);
    }

    #[test]
    fn an_agent_of_a_settled_worker_only_records_its_exit_code() {
        let f = fixture("done");
        assert_eq!(
            agent_exited(&f.db, &f.worker, Some(7)).unwrap(),
            Ended::Clean
        );
        assert_eq!(row(&f), ("done".into(), None, Some(7)));
        assert_eq!(exited_text(&f), ["agent exited with code 7"]);
    }

    #[test]
    fn trf_49_same_error_prints_once_then_every_10_min() {
        let mut log = CheckinLog::default();
        let t0 = Instant::now();
        let err = || Err("the task ledger is schema version 8".to_string());
        assert_eq!(
            log.record(err(), t0).as_deref(),
            Some("horch worker: checkin failed: the task ledger is schema version 8")
        );
        // 1 checkin each 30 s for 10 min: silent until the 10 min mark.
        let mut printed = Vec::new();
        for i in 1..=40u64 {
            if let Some(line) = log.record(err(), t0 + Duration::from_secs(30 * i)) {
                printed.push((i, line));
            }
        }
        assert_eq!(
            printed,
            vec![
                (
                    20,
                    "horch worker: checkin failed 21 times in a row: the task ledger is schema version 8"
                        .to_string()
                ),
                (
                    40,
                    "horch worker: checkin failed 41 times in a row: the task ledger is schema version 8"
                        .to_string()
                ),
            ]
        );
    }

    #[test]
    fn trf_49_changed_error_prints() {
        let mut log = CheckinLog::default();
        let t0 = Instant::now();
        assert!(log.record(Err("a".into()), t0).is_some());
        assert!(log
            .record(Err("a".into()), t0 + Duration::from_secs(1))
            .is_none());
        assert_eq!(
            log.record(Err("b".into()), t0 + Duration::from_secs(2))
                .as_deref(),
            Some("horch worker: checkin failed: b")
        );
        assert!(log
            .record(Err("b".into()), t0 + Duration::from_secs(3))
            .is_none());
        // Back to the first text: it changed again, so it prints.
        assert_eq!(
            log.record(Err("a".into()), t0 + Duration::from_secs(4))
                .as_deref(),
            Some("horch worker: checkin failed: a")
        );
    }

    #[test]
    fn trf_49_recovery_prints_1_line() {
        let mut log = CheckinLog::default();
        let t0 = Instant::now();
        assert_eq!(log.record(Ok(()), t0), None, "no failure, no line");
        for (i, text) in ["a", "a", "b"].into_iter().enumerate() {
            log.record(Err(text.into()), t0 + Duration::from_secs(i as u64));
        }
        assert_eq!(
            log.record(Ok(()), t0 + Duration::from_secs(5)).as_deref(),
            Some("horch worker: checkin recovered after 3 failures")
        );
        assert_eq!(log.record(Ok(()), t0 + Duration::from_secs(6)), None);
        // A new failure after the recovery prints again.
        assert_eq!(
            log.record(Err("a".into()), t0 + Duration::from_secs(7))
                .as_deref(),
            Some("horch worker: checkin failed: a")
        );
    }
}
