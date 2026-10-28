//! Stopping 1 worker by its recorded PIDs and its exact pane (section 7,
//! docs/adr/0002, #35).
//!
//! [`stop_worker`] takes a worker id and nothing else that names a process.
//! It reads each PID and start time from the worker's row, and every signal
//! goes through [`crate::procid::stop_same`], which sends nothing when the
//! PID now belongs to another process. Every step leaves a `stops` row.

use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

use super::model::{
    Actor, EventKind, StopRequester, StopResult, StopSignal, StopTarget, WorkerKind, WorkerStatus,
};
use super::store::Store;
use super::tasks;
use super::util::{self, About};
use super::workers::{self, Worker};
use crate::procid;

/// How a stop reaches herdr: close 1 pane by its id.
pub trait Panes {
    /// Whether the pane exists.
    fn exists(&self, pane: &str) -> bool;
    /// Close the pane.
    fn close(&self, pane: &str) -> Result<()>;
}

/// The operating system side of a stop, replaceable in tests.
pub trait Processes {
    /// Whether `pid` runs with start time `started`.
    fn is_same(&self, pid: u32, started: u64) -> bool;
    /// Whether any process has `pid`.
    fn exists(&self, pid: u32) -> bool;
    /// Stop `pid` only while it runs with start time `started`.
    fn stop_same(&self, pid: u32, started: u64, how: procid::Stop) -> bool;
}

/// The real processes of this machine.
pub struct Os;

impl Processes for Os {
    fn is_same(&self, pid: u32, started: u64) -> bool {
        procid::is_same(pid, started)
    }

    fn exists(&self, pid: u32) -> bool {
        crate::fsx::pid_alive(pid)
    }

    fn stop_same(&self, pid: u32, started: u64, how: procid::Stop) -> bool {
        procid::stop_same(pid, started, how)
    }
}

/// 1 step of a stop, as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StopStep {
    pub target: String,
    pub pid: Option<i64>,
    pub signal: Option<String>,
    pub pane_id: Option<String>,
    pub result: String,
}

/// What [`stop_worker`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StopReport {
    pub worker_id: String,
    pub role: Option<String>,
    pub steps: Vec<StopStep>,
    /// The task the worker held, queued again.
    pub requeued: Option<String>,
}

/// Options of a stop.
#[derive(Debug, Clone, Copy)]
pub struct StopOptions {
    /// How long a process gets after TERM before KILL.
    pub grace: Duration,
    /// How often the grace wait looks.
    pub poll: Duration,
    /// The final state of the worker: `stopped` for a deliberate stop,
    /// `incomplete` for a reap.
    pub final_status: WorkerStatus,
}

impl Default for StopOptions {
    fn default() -> Self {
        StopOptions {
            grace: Duration::from_secs(10),
            poll: Duration::from_millis(100),
            final_status: WorkerStatus::Stopped,
        }
    }
}

fn record(
    conn: &Connection,
    w: &Worker,
    by: StopRequester,
    reason: &str,
    step: &StopStep,
    started: Option<i64>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO stops (worker_id, requested_by, reason, target, pid, pid_started, signal,
            pane_id, result, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            w.worker_id,
            by.as_str(),
            reason,
            step.target,
            step.pid,
            started,
            step.signal,
            step.pane_id,
            step.result,
            util::now()
        ],
    )?;
    Ok(())
}

/// Stop 1 process: TERM, wait up to the grace period, then KILL if the same
/// process still runs. Returns the steps taken.
fn stop_process(
    procs: &dyn Processes,
    target: StopTarget,
    pid: Option<i64>,
    started: Option<i64>,
    opts: &StopOptions,
) -> Vec<StopStep> {
    let step = |signal: Option<StopSignal>, result: StopResult| StopStep {
        target: target.as_str().into(),
        pid,
        signal: signal.map(|s| s.as_str().into()),
        pane_id: None,
        result: result.as_str().into(),
    };
    let Some(pid) = pid.and_then(|p| u32::try_from(p).ok()) else {
        return vec![step(None, StopResult::AlreadyGone)];
    };
    let Some(started) = started.and_then(|s| u64::try_from(s).ok()) else {
        // No recorded start time: the PID cannot be proved to be this
        // worker's process, so nothing is sent.
        return vec![step(None, StopResult::NotSameProcess)];
    };
    if !procs.is_same(pid, started) {
        // The PID is free, or another program has it now: nothing is sent.
        return vec![step(
            None,
            if procs.exists(pid) {
                StopResult::NotSameProcess
            } else {
                StopResult::AlreadyGone
            },
        )];
    }
    let mut steps = Vec::new();
    if procs.stop_same(pid, started, procid::Stop::Term) {
        steps.push(step(Some(StopSignal::Term), StopResult::Sent));
    } else {
        steps.push(step(Some(StopSignal::Term), StopResult::NotSameProcess));
        return steps;
    }
    let until = Instant::now() + opts.grace;
    while Instant::now() < until {
        if !procs.is_same(pid, started) {
            return steps;
        }
        std::thread::sleep(opts.poll);
    }
    if procs.is_same(pid, started) {
        let sent = procs.stop_same(pid, started, procid::Stop::Kill);
        steps.push(step(
            Some(StopSignal::Kill),
            if sent {
                StopResult::Sent
            } else {
                StopResult::AlreadyGone
            },
        ));
    }
    steps
}

/// Stop worker `worker_id`: its agent, then its supervisor, each by the PID
/// and start time in its row, then its pane by its id. The worker's state
/// becomes `opts.final_status` first, so its supervisor does not report the
/// exit as a crash, and a task it held goes back to the queue.
pub fn stop_worker(
    store: &mut Store,
    worker_id: &str,
    by: StopRequester,
    reason: &str,
    panes: &dyn Panes,
    procs: &dyn Processes,
    opts: &StopOptions,
) -> Result<StopReport> {
    let w = workers::get(store.conn(), worker_id)?;
    if w.kind == WorkerKind::Orchestrator.as_str() {
        bail!("horch ledger stop does not stop the orchestrator; the operator does");
    }
    let here = util::host_name();
    if w.host.as_deref().is_some_and(|h| h != here) {
        let step = StopStep {
            target: StopTarget::Agent.as_str().into(),
            pid: w.agent_pid,
            signal: None,
            pane_id: None,
            result: StopResult::RefusedOtherHost.as_str().into(),
        };
        record(store.conn(), &w, by, reason, &step, w.agent_started)?;
        bail!(
            "worker {} runs on host {}, not {here}: its PIDs mean nothing here",
            w.label(),
            w.host.as_deref().unwrap_or("?")
        );
    }
    // Settle the row first: the supervisor sees a worker that is no longer
    // working and records only the exit code.
    let requeued = store.immediate(|tx| {
        let about = About {
            run: w.run_id.as_deref(),
            task: w.task_id.as_deref(),
            worker: Some(worker_id),
        };
        workers::end(tx, worker_id, opts.final_status, reason, None)?;
        util::event(
            tx,
            &about,
            match by {
                StopRequester::Watchdog => Actor::Watchdog,
                StopRequester::SelfExit => Actor::Supervisor,
                StopRequester::Operator => Actor::Operator,
                StopRequester::Orchestrator => Actor::Orchestrator,
            },
            EventKind::Stopped,
            &format!("Stopping {}: {reason}", w.label()),
        )?;
        let mut requeued = None;
        if let Some(t) = &w.task_id {
            let task = tasks::get(tx, t)?;
            if matches!(task.status.as_str(), "running" | "landing")
                && task.current_worker_id.as_deref() == Some(worker_id)
            {
                tasks::requeue(tx, t, &format!("{} was stopped: {reason}", w.label()))?;
                requeued = Some(task.short());
            }
        }
        Ok(requeued)
    })?;
    let mut steps = Vec::new();
    for (target, pid, started) in [
        (StopTarget::Agent, w.agent_pid, w.agent_started),
        (
            StopTarget::Supervisor,
            w.supervisor_pid,
            w.supervisor_started,
        ),
    ] {
        for s in stop_process(procs, target, pid, started, opts) {
            record(store.conn(), &w, by, reason, &s, started)?;
            steps.push(s);
        }
    }
    let pane_step = match &w.pane_id {
        Some(pane) if panes.exists(pane) => {
            let result = match panes.close(pane) {
                Ok(()) => StopResult::PaneClosed,
                Err(_) => StopResult::Error,
            };
            StopStep {
                target: StopTarget::Pane.as_str().into(),
                pid: None,
                signal: None,
                pane_id: Some(pane.clone()),
                result: result.as_str().into(),
            }
        }
        other => StopStep {
            target: StopTarget::Pane.as_str().into(),
            pid: None,
            signal: None,
            pane_id: other.clone(),
            result: StopResult::AlreadyGone.as_str().into(),
        },
    };
    record(store.conn(), &w, by, reason, &pane_step, None)?;
    steps.push(pane_step);
    Ok(StopReport {
        worker_id: worker_id.to_string(),
        role: w.role.clone(),
        steps,
        requeued,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;

    use super::*;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::workers::NewWorker;

    struct NoPanes;
    impl Panes for NoPanes {
        fn exists(&self, _: &str) -> bool {
            false
        }
        fn close(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }

    /// A process that ignores TERM.
    struct Stubborn {
        alive: RefCell<bool>,
        sent: RefCell<Vec<procid::Stop>>,
    }
    impl Processes for Stubborn {
        fn is_same(&self, _: u32, started: u64) -> bool {
            started == 7 && *self.alive.borrow()
        }
        fn exists(&self, _: u32) -> bool {
            *self.alive.borrow()
        }
        fn stop_same(&self, pid: u32, started: u64, how: procid::Stop) -> bool {
            if !self.is_same(pid, started) {
                return false;
            }
            self.sent.borrow_mut().push(how);
            if how == procid::Stop::Kill {
                *self.alive.borrow_mut() = false;
            }
            true
        }
    }

    fn worker(store: &mut Store, started: i64) -> String {
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
                let id = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id,
                        persona: "opus".into(),
                        ..Default::default()
                    },
                )?;
                tx.execute(
                    "UPDATE workers SET agent_pid = 4000, agent_started = ?2, status = 'claiming'
                     WHERE worker_id = ?1",
                    params![id, started],
                )?;
                Ok(id)
            })
            .unwrap()
    }

    #[test]
    fn a_process_that_ignores_term_gets_kill_after_the_grace() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let id = worker(&mut s, 7);
        let procs = Stubborn {
            alive: RefCell::new(true),
            sent: RefCell::new(vec![]),
        };
        let opts = StopOptions {
            grace: Duration::from_millis(50),
            poll: Duration::from_millis(10),
            ..Default::default()
        };
        let r = stop_worker(
            &mut s,
            &id,
            StopRequester::Orchestrator,
            "test",
            &NoPanes,
            &procs,
            &opts,
        )
        .unwrap();
        assert_eq!(
            *procs.sent.borrow(),
            vec![procid::Stop::Term, procid::Stop::Kill]
        );
        let agent: Vec<&StopStep> = r.steps.iter().filter(|s| s.target == "agent").collect();
        assert_eq!(agent.len(), 2);
        assert_eq!(agent[1].signal.as_deref(), Some("KILL"));
        assert_eq!(workers::get(s.conn(), &id).unwrap().status, "stopped");
    }

    #[test]
    fn a_reused_pid_is_not_signalled() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        // The recorded start time (8) is not the running process's (7).
        let id = worker(&mut s, 8);
        let procs = Stubborn {
            alive: RefCell::new(true),
            sent: RefCell::new(vec![]),
        };
        let r = stop_worker(
            &mut s,
            &id,
            StopRequester::Orchestrator,
            "t",
            &NoPanes,
            &procs,
            &StopOptions::default(),
        )
        .unwrap();
        assert!(procs.sent.borrow().is_empty());
        assert_eq!(r.steps[0].result, "not_same_process");
    }
}

/// What [`reap`] did: the stop, and the persona to spawn for the requeued
/// task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReapReport {
    pub stop: StopReport,
    pub persona: Option<String>,
}

/// Reap a dead worker (section 5.4): stop it by its recorded PIDs and pane,
/// end its attempt `incomplete`, and queue its task again, first in line.
/// A worker that already finished (`done`, `no_task`, `stopped`) is refused:
/// there is nothing to reap.
pub fn reap(
    store: &mut Store,
    worker_id: &str,
    by: StopRequester,
    reason: &str,
    panes: &dyn Panes,
    procs: &dyn Processes,
    grace: Duration,
) -> Result<ReapReport> {
    let w = workers::get(store.conn(), worker_id)?;
    let status = w.status()?;
    if !status.is_live() && status != WorkerStatus::Incomplete {
        bail!(
            "worker {} is {}; only a live or incomplete worker is reaped",
            w.label(),
            w.status
        );
    }
    let opts = StopOptions {
        grace,
        final_status: WorkerStatus::Incomplete,
        ..Default::default()
    };
    let stop = stop_worker(
        store,
        worker_id,
        by,
        &format!("dead: {reason}"),
        panes,
        procs,
        &opts,
    )?;
    // A worker whose supervisor already settled it (incomplete, task
    // requeued) still gets its pane closed above; the task is checked here.
    let persona = match &w.task_id {
        Some(t) => {
            let task = tasks::get(store.conn(), t)?;
            if matches!(task.status.as_str(), "running" | "landing")
                && task.current_worker_id.as_deref() == Some(worker_id)
            {
                store.immediate(|tx| tasks::requeue(tx, t, reason))?;
            }
            task.claimable_by
        }
        None => w.persona.clone(),
    };
    util::event(
        store.conn(),
        &About {
            run: w.run_id.as_deref(),
            task: w.task_id.as_deref(),
            worker: Some(worker_id),
        },
        match by {
            StopRequester::Watchdog => Actor::Watchdog,
            _ => Actor::Orchestrator,
        },
        EventKind::Reaped,
        &format!("{} reaped: {reason}", w.label()),
    )?;
    Ok(ReapReport { stop, persona })
}
