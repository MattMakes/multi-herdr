//! The takeover of the old watchdogs at the start of fleetd.
//!
//! `docs/specs/fleetd/design.md`, "Migration plan" item 3 ("Phase 1 stops
//! the old watchdog"), and the table "What a release does to a run in
//! flight", row Phase 1. Before Phase 1, each active run had 1 watchdog
//! process (`horch ledger watch`). The run records it in 2 places:
//!
//! - the owner file of the lock folder `<locks>/watchdog-<run>.lock`
//!   (`taskledger::watchdog::lock`);
//! - the columns `runs.watchdog_pid` and `runs.watchdog_started`
//!   (`taskledger::watchdog::record_start` and `checkin`).
//!
//! At each start, before its first pass, fleetd calls [`take_over`]. For
//! each recorded watchdog of each active run:
//!
//! - when the process with the recorded PID has the recorded start time,
//!   fleetd stops it: a polite stop, then a kill after `stop_grace_s`;
//! - when a process has the recorded PID but the record has no start time,
//!   fleetd cannot tell the watchdog from another program with a reused
//!   PID. It sends no signal, and the record stays;
//! - else the recorded watchdog is gone, and fleetd sends no signal. A PID
//!   with another start time names another program.
//!
//! Then 1 transaction checks the generation of this fleetd, clears the
//! record of each run whose watchdogs are gone, and writes 1 event with
//! the cause [`CAUSE`] for each recorded watchdog that is gone. Last, the
//! lock folder of each watchdog that no longer runs is removed.
//!
//! 1 control path: fleetd runs a pass only when [`all_gone`] holds for the
//! result. Else it runs no pass, and its next start tries again. The pass
//! then does the duties of the watchdog for the run. A live worker goes
//! on: the takeover touches only the watchdog.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::fleetd::fleet_runtime::adapters::fleetd_row::check_generation;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::procid;
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::runs::Run;
use crate::taskledger::store::Store;
use crate::taskledger::{util, watchdog};

/// The cause of the event of each recorded watchdog that fleetd took over.
pub const CAUSE: &str = "old_watchdog_stopped";

/// The time between 2 reads of a process that fleetd stops.
const WAIT_STEP: Duration = Duration::from_millis(50);

/// How long fleetd waits for the end of a process after its kill.
const KILL_WAIT: Duration = Duration::from_secs(5);

/// The processes of this machine, as the takeover reads and stops them.
pub trait Processes {
    /// The start time of the process `pid`, or `None` when no process has
    /// the pid or its start time cannot be read.
    fn start_time(&self, pid: u32) -> Option<u64>;

    /// Whether a process has the pid `pid`.
    fn exists(&self, pid: u32) -> bool {
        self.start_time(pid).is_some()
    }

    /// Stop `pid` while it runs with start time `started`: a polite stop,
    /// then a kill after `grace`. True when the process ended.
    fn stop(&self, pid: u32, started: u64, grace: Duration) -> bool;
}

/// The processes of this machine, through the platform layer.
#[derive(Debug, Clone, Copy, Default)]
pub struct Os;

impl Processes for Os {
    fn start_time(&self, pid: u32) -> Option<u64> {
        procid::alive(pid, None)
            .then(|| procid::start_time(pid))
            .flatten()
    }

    fn exists(&self, pid: u32) -> bool {
        procid::alive(pid, None)
    }

    fn stop(&self, pid: u32, started: u64, grace: Duration) -> bool {
        procid::stop_same(pid, started, procid::Stop::Term);
        if ended(pid, started, grace) {
            return true;
        }
        procid::stop_same(pid, started, procid::Stop::Kill);
        ended(pid, started, KILL_WAIT)
    }
}

/// Wait up to `wait` until `pid` with start time `started` has ended.
fn ended(pid: u32, started: u64, wait: Duration) -> bool {
    let until = Instant::now() + wait;
    loop {
        if !procid::alive(pid, Some(started)) {
            return true;
        }
        let now = Instant::now();
        if now >= until {
            return false;
        }
        std::thread::sleep(WAIT_STEP.min(until - now));
    }
}

/// What fleetd did about 1 recorded watchdog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// It ran, and fleetd stopped it by its PID and start time.
    Stopped,
    /// It did not run: fleetd cleared its record with no signal.
    NotRunning,
    /// It still runs after its kill. Its record stays, and the next start
    /// of fleetd tries again.
    StillRuns,
    /// A process has its PID, but the record has no start time: fleetd
    /// cannot tell it from another program with a reused PID. It sends no
    /// signal, the record stays, and the next start of fleetd tries again.
    Unconfirmed,
}

impl Outcome {
    /// Whether the recorded watchdog no longer runs.
    pub fn gone(self) -> bool {
        matches!(self, Outcome::Stopped | Outcome::NotRunning)
    }
}

/// Whether each recorded watchdog of `taken` no longer runs: fleetd may run
/// its passes with 1 control path.
pub fn all_gone(taken: &[TakenOver]) -> bool {
    taken.iter().all(|t| t.outcome.gone())
}

/// 1 recorded watchdog of an active run, and what fleetd did about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakenOver {
    pub run_id: String,
    pub slug: String,
    pub pid: u32,
    /// The recorded start time. `None` when the record has none: then
    /// fleetd cannot prove the process is the watchdog, and sends no
    /// signal.
    pub pid_start: Option<u64>,
    pub outcome: Outcome,
}

impl TakenOver {
    /// 1 line for the output of fleetd.
    pub fn describe(&self) -> String {
        match self.outcome {
            Outcome::Stopped => format!(
                "fleetd stopped the old watchdog of run {} (PID {}) and takes its run.",
                self.slug, self.pid
            ),
            Outcome::NotRunning => format!(
                "The old watchdog of run {} (PID {}) does not run: fleetd cleared its record.",
                self.slug, self.pid
            ),
            Outcome::StillRuns => format!(
                "The old watchdog of run {} (PID {}) still runs after its kill: its record stays.",
                self.slug, self.pid
            ),
            Outcome::Unconfirmed => format!(
                "A process has the PID {} of the old watchdog of run {}, but the record has no start time: fleetd sends no signal, and its record stays. Stop that watchdog if it runs.",
                self.pid, self.slug
            ),
        }
    }
}

/// The lock folder of the watchdog of `run_id` in `locks`.
fn lock_folder(locks: &Path, run_id: &str) -> PathBuf {
    locks.join(format!("watchdog-{run_id}.lock"))
}

/// The process that the owner file of the lock folder `lock` names: its
/// PID and its start time.
fn lock_owner(lock: &Path) -> Option<(u32, Option<u64>)> {
    let v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(lock.join("owner")).ok()?).ok()?;
    let pid = u32::try_from(v["pid"].as_u64()?).ok()?;
    Some((pid, v["started"].as_u64()))
}

/// The watchdog processes that `run` records, each 1 time: the lock owner
/// first, then the columns of the run.
fn recorded(run: &Run, locks: &Path) -> Vec<(u32, Option<u64>)> {
    let columns = run.watchdog_pid.and_then(|pid| {
        let pid = u32::try_from(pid).ok()?;
        Some((
            pid,
            run.watchdog_started.and_then(|s| u64::try_from(s).ok()),
        ))
    });
    let mut found = Vec::new();
    for p in lock_owner(&lock_folder(locks, &run.run_id))
        .into_iter()
        .chain(columns)
    {
        if !found.contains(&p) {
            found.push(p);
        }
    }
    found
}

/// Whether `pid` runs with the start time `started`.
fn runs(procs: &dyn Processes, pid: u32, started: Option<u64>) -> bool {
    started.is_some_and(|s| procs.start_time(pid) == Some(s))
}

/// Whether the process `pid` with the recorded start time `started` can
/// still run. With no start time, any process with the pid can be it.
fn may_run(procs: &dyn Processes, pid: u32, started: Option<u64>) -> bool {
    match started {
        Some(_) => runs(procs, pid, started),
        None => procs.exists(pid),
    }
}

/// Stop the recorded watchdog `pid` when it runs with its recorded start
/// time `started`.
fn stop_one(procs: &dyn Processes, pid: u32, started: Option<u64>, grace: Duration) -> Outcome {
    match started {
        Some(s) if runs(procs, pid, started) => {
            if procs.stop(pid, s, grace) {
                Outcome::Stopped
            } else {
                Outcome::StillRuns
            }
        }
        None if procs.exists(pid) => Outcome::Unconfirmed,
        _ => Outcome::NotRunning,
    }
}

/// Take over each old watchdog of the active runs of `store`, as the
/// module doc says. `locks` is the lock folder of the ledger, and `grace`
/// is `stop_grace_s`. Returns 1 item for each recorded watchdog.
pub fn take_over(
    store: &mut Store,
    generation: Generation,
    locks: &Path,
    grace: Duration,
    procs: &dyn Processes,
) -> Result<Vec<TakenOver>> {
    let mut taken = Vec::new();
    for run in watchdog::active_runs(store.conn())? {
        for (pid, pid_start) in recorded(&run, locks) {
            taken.push(TakenOver {
                run_id: run.run_id.clone(),
                slug: run.slug.clone(),
                pid,
                pid_start,
                outcome: stop_one(procs, pid, pid_start, grace),
            });
        }
    }
    if taken.is_empty() {
        return Ok(taken);
    }
    // A run with a watchdog that may still run keeps its record.
    let kept: BTreeSet<&str> = taken
        .iter()
        .filter(|t| !t.outcome.gone())
        .map(|t| t.run_id.as_str())
        .collect();
    store.immediate(|tx| {
        check_generation(tx, generation)?;
        for t in taken.iter().filter(|t| t.outcome.gone()) {
            if !kept.contains(t.run_id.as_str()) {
                watchdog::clear_record(tx, &t.run_id)?;
            }
            event(tx, t)?;
        }
        Ok(())
    })?;
    for t in &taken {
        let lock = lock_folder(locks, &t.run_id);
        let free = lock_owner(&lock).is_some_and(|(pid, started)| !may_run(procs, pid, started));
        if free {
            remove_lock(&lock)?;
        }
    }
    Ok(taken)
}

/// Remove the lock folder `lock` of a watchdog that no longer runs. A
/// watchdog that ended on its polite stop removes it itself.
fn remove_lock(lock: &Path) -> Result<()> {
    match std::fs::remove_dir_all(lock) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", lock.display()))
        }
        _ => Ok(()),
    }
}

/// The event of the takeover of 1 recorded watchdog.
fn event(tx: &Connection, t: &TakenOver) -> Result<()> {
    let data = serde_json::json!({
        "pid": t.pid,
        "pid_start": t.pid_start,
        "signalled": t.outcome == Outcome::Stopped,
    })
    .to_string();
    tx.execute(
        "INSERT INTO events (at, run_id, actor, kind, text, cause, data_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            util::now(),
            t.run_id,
            Actor::Horch.as_str(),
            EventKind::Stopped.as_str(),
            t.describe(),
            CAUSE,
            data
        ],
    )
    .with_context(|| format!("recording the takeover of the watchdog of run {}", t.slug))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::fleetd::fleet_runtime::adapters::fleetd_row;
    use crate::taskledger::runs::{self, NewRun};

    /// The processes of a test: a map of PID to start time, and the stops.
    #[derive(Default)]
    struct Fake {
        live: RefCell<Vec<(u32, u64)>>,
        stops: RefCell<Vec<(u32, u64, Duration)>>,
        /// Each process survives its stop.
        unkillable: bool,
    }

    impl Processes for Fake {
        fn start_time(&self, pid: u32) -> Option<u64> {
            let live = self.live.borrow();
            live.iter().find(|(p, _)| *p == pid).map(|(_, s)| *s)
        }

        fn stop(&self, pid: u32, started: u64, grace: Duration) -> bool {
            self.stops.borrow_mut().push((pid, started, grace));
            if self.unkillable {
                return false;
            }
            self.live.borrow_mut().retain(|p| *p != (pid, started));
            true
        }
    }

    struct World {
        dir: tempfile::TempDir,
        store: Store,
        generation: Generation,
        run_id: String,
    }

    impl World {
        fn new() -> World {
            let dir = tempfile::tempdir().unwrap();
            let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
            let started = fleetd_row::start(&dir.path().join("fleet"), &mut store)
                .unwrap()
                .expect("the fleetd lock");
            let new = NewRun {
                slug: "zeta".into(),
                ..Default::default()
            };
            let run = store
                .immediate(|tx| runs::create(tx, dir.path(), &new))
                .unwrap();
            World {
                dir,
                store,
                generation: started.generation,
                run_id: run.run_id,
            }
        }

        fn locks(&self) -> PathBuf {
            self.dir.path().join("locks")
        }

        /// Record the watchdog `pid` with start time `started` as the lock
        /// owner and on the run.
        fn record(&self, pid: u32, started: u64) {
            let lock = lock_folder(&self.locks(), &self.run_id);
            std::fs::create_dir_all(&lock).unwrap();
            let owner = serde_json::json!({ "pid": pid, "started": started });
            std::fs::write(lock.join("owner"), owner.to_string()).unwrap();
            watchdog::record_start(self.store.conn(), &self.run_id, pid, Some(started)).unwrap();
        }

        /// Record `pid` as the watchdog of the run with no start time, as a
        /// starter that could not read it wrote it.
        fn record_no_start(&self, pid: u32) {
            let lock = lock_folder(&self.locks(), &self.run_id);
            std::fs::create_dir_all(&lock).unwrap();
            let owner = serde_json::json!({ "pid": pid });
            std::fs::write(lock.join("owner"), owner.to_string()).unwrap();
            watchdog::record_start(self.store.conn(), &self.run_id, pid, None).unwrap();
        }

        fn take_over(&mut self, procs: &Fake) -> Vec<TakenOver> {
            let locks = self.locks();
            take_over(
                &mut self.store,
                self.generation,
                &locks,
                Duration::from_secs(7),
                procs,
            )
            .unwrap()
        }

        /// The recorded watchdog columns of the run.
        fn columns(&self) -> (Option<i64>, Option<i64>) {
            let run = runs::get(self.store.conn(), &self.run_id).unwrap();
            (run.watchdog_pid, run.watchdog_started)
        }

        /// The `data_json` of each event with the cause of the takeover.
        fn events(&self) -> Vec<serde_json::Value> {
            let mut stmt = self
                .store
                .conn()
                .prepare("SELECT data_json FROM events WHERE cause = ?1 AND run_id = ?2")
                .unwrap();
            stmt.query_map([CAUSE, self.run_id.as_str()], |r| r.get::<_, String>(0))
                .unwrap()
                .map(|d| serde_json::from_str(&d.unwrap()).unwrap())
                .collect()
        }
    }

    #[test]
    fn first_start_stops_a_recorded_watchdog_by_pid_and_start_time() {
        let mut w = World::new();
        w.record(4242, 77);
        let procs = Fake::default();
        procs.live.borrow_mut().push((4242, 77));
        let taken = w.take_over(&procs);
        assert_eq!(
            *procs.stops.borrow(),
            vec![(4242, 77, Duration::from_secs(7))],
            "1 stop by the PID and the start time, with stop_grace_s"
        );
        assert_eq!(taken.len(), 1, "{taken:?}");
        assert_eq!(taken[0].outcome, Outcome::Stopped);
        assert!(all_gone(&taken), "fleetd may run its passes");
        assert_eq!(w.columns(), (None, None), "the record is cleared");
        assert!(!lock_folder(&w.locks(), &w.run_id).exists());
        let events = w.events();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["pid"], 4242);
        assert_eq!(events[0]["signalled"], true);
    }

    #[test]
    fn a_pid_with_another_start_time_is_not_signalled() {
        let mut w = World::new();
        w.record(4242, 77);
        let procs = Fake::default();
        // The PID names a later program.
        procs.live.borrow_mut().push((4242, 78));
        let taken = w.take_over(&procs);
        assert!(
            procs.stops.borrow().is_empty(),
            "{:?}",
            procs.stops.borrow()
        );
        assert_eq!(procs.start_time(4242), Some(78), "the other program runs");
        assert_eq!(taken[0].outcome, Outcome::NotRunning);
        assert_eq!(w.columns(), (None, None), "the record is cleared");
        assert_eq!(w.events()[0]["signalled"], false);
    }

    #[test]
    fn a_dead_recorded_watchdog_is_cleared_with_no_signal() {
        let mut w = World::new();
        w.record(4242, 77);
        let procs = Fake::default();
        let taken = w.take_over(&procs);
        assert!(
            procs.stops.borrow().is_empty(),
            "{:?}",
            procs.stops.borrow()
        );
        assert_eq!(taken.len(), 1, "{taken:?}");
        assert_eq!(taken[0].outcome, Outcome::NotRunning);
        assert_eq!(w.columns(), (None, None), "the record is cleared");
        assert!(!lock_folder(&w.locks(), &w.run_id).exists());
        let events = w.events();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["signalled"], false);
        // A second start finds nothing to take over.
        assert!(w.take_over(&procs).is_empty());
    }

    #[test]
    fn a_watchdog_that_survives_its_kill_keeps_its_record_and_blocks_the_passes() {
        let mut w = World::new();
        w.record(4242, 77);
        let procs = Fake {
            unkillable: true,
            ..Fake::default()
        };
        procs.live.borrow_mut().push((4242, 77));
        let taken = w.take_over(&procs);
        assert_eq!(procs.stops.borrow().len(), 1, "1 stop");
        assert_eq!(taken[0].outcome, Outcome::StillRuns);
        assert!(!all_gone(&taken), "1 control path: fleetd runs no pass");
        assert_eq!(w.columns(), (Some(4242), Some(77)), "the record stays");
        assert!(lock_folder(&w.locks(), &w.run_id).exists());
        assert!(w.events().is_empty(), "no takeover event");
    }

    #[test]
    fn a_live_pid_with_no_recorded_start_time_is_not_signalled_and_blocks_the_passes() {
        let mut w = World::new();
        w.record_no_start(4242);
        let procs = Fake::default();
        // A process has the PID: the watchdog, or a program with a reused PID.
        procs.live.borrow_mut().push((4242, 78));
        let taken = w.take_over(&procs);
        assert!(
            procs.stops.borrow().is_empty(),
            "{:?}",
            procs.stops.borrow()
        );
        assert_eq!(taken.len(), 1, "{taken:?}");
        assert_eq!(taken[0].outcome, Outcome::Unconfirmed);
        assert!(!all_gone(&taken), "1 control path: fleetd runs no pass");
        assert_eq!(w.columns(), (Some(4242), None), "the record stays");
        assert!(lock_folder(&w.locks(), &w.run_id).exists());
        assert!(w.events().is_empty(), "no takeover event");
        // Once no process has the PID, the next start clears the record.
        procs.live.borrow_mut().clear();
        let taken = w.take_over(&procs);
        assert_eq!(taken[0].outcome, Outcome::NotRunning);
        assert!(all_gone(&taken));
        assert_eq!(w.columns(), (None, None));
    }
}
