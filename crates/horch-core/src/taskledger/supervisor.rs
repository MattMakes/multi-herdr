//! What `horch worker` (the supervisor) writes to the task ledger
//! (section 4.6, #34): its own PID, its agent's PID, a checkin every
//! `heartbeat_every_s`, the session and transcript once known, and the end.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

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
        loop {
            if flag.load(Ordering::SeqCst) {
                return;
            }
            if since >= every {
                since = Duration::ZERO;
                if let Err(e) = beat(&db, &worker_id, &lookup, &mut session_known) {
                    eprintln!("horch worker: checkin failed: {e:#}");
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
    s.immediate(|tx| {
        let w = workers::get(tx, worker_id)?;
        let how = match code {
            Some(c) => format!("agent exited with code {c}"),
            None => "agent was ended by a signal".to_string(),
        };
        let status = w.status()?;
        let about = About {
            run: w.run_id.as_deref(),
            task: w.task_id.as_deref(),
            worker: Some(worker_id),
        };
        match status {
            WorkerStatus::Working => {
                let reason = format!("{how} before horch ledger done");
                workers::end(tx, worker_id, WorkerStatus::Incomplete, &reason, code)?;
                util::event(tx, &about, Actor::Supervisor, EventKind::Exited, &reason)?;
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
            WorkerStatus::Starting | WorkerStatus::Spawning => {
                workers::end(tx, worker_id, WorkerStatus::LaunchFailed, &how, code)?;
                util::event(tx, &about, Actor::Supervisor, EventKind::Exited, &how)?;
                Ok(Ended::Clean)
            }
            WorkerStatus::Claiming => {
                let reason = format!("{how} before it claimed a task");
                workers::end(tx, worker_id, WorkerStatus::NoTask, &reason, code)?;
                util::event(tx, &about, Actor::Supervisor, EventKind::Exited, &reason)?;
                Ok(Ended::Clean)
            }
            _ => {
                workers::set_exit_code(tx, worker_id, code)?;
                util::event(tx, &about, Actor::Supervisor, EventKind::Exited, &how)?;
                Ok(Ended::Clean)
            }
        }
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
        assert_eq!(
            row(&f),
            ("incomplete".into(), Some(reason.into()), Some(3))
        );
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
        assert_eq!(agent_exited(&f.db, &f.worker, Some(0)).unwrap(), Ended::Clean);
        let reason = "agent exited with code 0 before it claimed a task";
        assert_eq!(row(&f), ("no_task".into(), Some(reason.into()), Some(0)));
        assert_eq!(exited_text(&f), [reason]);
    }

    #[test]
    fn an_agent_of_a_settled_worker_only_records_its_exit_code() {
        let f = fixture("done");
        assert_eq!(agent_exited(&f.db, &f.worker, Some(7)).unwrap(), Ended::Clean);
        assert_eq!(row(&f), ("done".into(), None, Some(7)));
        assert_eq!(exited_text(&f), ["agent exited with code 7"]);
    }
}
