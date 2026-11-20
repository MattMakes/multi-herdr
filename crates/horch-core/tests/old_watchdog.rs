//! The Phase 1 takeover of the old watchdogs stops only the processes that
//! its own ledger records (A1, `ai_docs/plans/fleetd-finish/plan.md`).
//!
//! 2 ledgers share 1 folder, so they also share the lock folder
//! `<folder>/locks` of `orchestrator::lock_dir`. Real processes stand in for
//! the watchdogs, and the takeover stops them through the platform
//! ([`old_watchdog::Os`]).

#![cfg(unix)]

use std::cell::RefCell;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::fleetd_row;
use horch_core::fleetd::fleet_runtime::adapters::old_watchdog::{
    self, Outcome, Processes, TakenOver,
};
use horch_core::procid;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::store::Store;
use horch_core::taskledger::{orchestrator, watchdog};

/// The processes of this machine, with a record of each stop.
#[derive(Default)]
struct Recording {
    stops: RefCell<Vec<(u32, u64)>>,
}

impl Processes for Recording {
    fn start_time(&self, pid: u32) -> Option<u64> {
        old_watchdog::Os.start_time(pid)
    }

    fn exists(&self, pid: u32) -> bool {
        old_watchdog::Os.exists(pid)
    }

    fn stop(&self, pid: u32, started: u64, grace: Duration) -> bool {
        self.stops.borrow_mut().push((pid, started));
        old_watchdog::Os.stop(pid, started, grace)
    }
}

/// A process that runs until a signal stops it.
fn sleeper() -> Child {
    Command::new("sleep")
        .arg("120")
        .stdin(Stdio::null())
        .spawn()
        .expect("start sleep")
}

/// The start time of `pid`, which runs.
fn started(pid: u32) -> u64 {
    procid::start_time(pid).expect("the start time of a live process")
}

/// A new active run in `store`, with `pid` and `started` as its recorded
/// watchdog on the run and in the owner file of its lock folder.
fn record(store: &mut Store, locks: &Path, slug: &str, pid: u32, started: u64) -> String {
    let new = NewRun {
        slug: slug.into(),
        ..Default::default()
    };
    let root = store.path().parent().unwrap().to_path_buf();
    let run = store.immediate(|tx| runs::create(tx, &root, &new)).unwrap();
    let lock = locks.join(format!("watchdog-{}.lock", run.run_id));
    std::fs::create_dir_all(&lock).unwrap();
    let owner = serde_json::json!({ "pid": pid, "started": started });
    std::fs::write(lock.join("owner"), owner.to_string()).unwrap();
    watchdog::record_start(store.conn(), &run.run_id, pid, Some(started)).unwrap();
    run.run_id
}

/// The recorded watchdog columns of `run_id` in `store`.
fn columns(store: &Store, run_id: &str) -> (Option<i64>, Option<i64>) {
    let run = runs::get(store.conn(), run_id).unwrap();
    (run.watchdog_pid, run.watchdog_started)
}

/// The exit status of the process that `waiter` reaps, within `wait`.
fn exited(waiter: &std::thread::JoinHandle<ExitStatus>, wait: Duration) -> bool {
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if waiter.is_finished() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    waiter.is_finished()
}

#[test]
fn take_over_stops_only_the_watchdog_that_its_own_ledger_records() {
    let dir = tempfile::tempdir().unwrap();
    let ledger_a = dir.path().join("a.sqlite3");
    let ledger_b = dir.path().join("b.sqlite3");
    let locks = orchestrator::lock_dir(&ledger_a);
    assert_eq!(locks, orchestrator::lock_dir(&ledger_b), "1 lock folder");

    let watchdog_a = sleeper();
    let mut other_b = sleeper();
    let (pid_a, pid_b) = (watchdog_a.id(), other_b.id());
    let (start_a, start_b) = (started(pid_a), started(pid_b));
    // The test reaps the stopped process at once, as init reaps a real
    // watchdog, so the stop sees it end.
    let mut watchdog_a = watchdog_a;
    let waiter_a = std::thread::spawn(move || watchdog_a.wait().unwrap());

    let mut a = Store::open(&ledger_a).unwrap();
    let mut b = Store::open(&ledger_b).unwrap();
    let fleetd_a = fleetd_row::start(&dir.path().join("fleet-a"), &mut a)
        .unwrap()
        .expect("the fleetd lock of ledger A");
    let run_a = record(&mut a, &locks, "alpha", pid_a, start_a);
    // Ledger A also names the live PID of ledger B, with another start
    // time: another program had that PID.
    let run_a_old = record(&mut a, &locks, "alpha-old", pid_b, start_b + 1);
    let run_b = record(&mut b, &locks, "beta", pid_b, start_b);

    let procs = Recording::default();
    let taken = old_watchdog::take_over(
        &mut a,
        fleetd_a.generation,
        &locks,
        Duration::from_secs(5),
        &procs,
    );
    let alive_b = procid::alive(pid_b, Some(start_b));
    let _ = other_b.kill();
    let _ = other_b.wait();
    let taken: Vec<TakenOver> = taken.unwrap();

    assert_eq!(
        *procs.stops.borrow(),
        vec![(pid_a, start_a)],
        "1 stop: the PID and start time that ledger A records"
    );
    assert!(
        exited(&waiter_a, Duration::from_secs(5)),
        "the watchdog of ledger A ended"
    );
    assert!(
        alive_b,
        "the process of ledger B still ran after the takeover"
    );
    let outcome = |run: &str| {
        taken
            .iter()
            .find(|t| t.run_id == run)
            .map(|t| t.outcome)
            .unwrap_or_else(|| panic!("no takeover of {run}: {taken:?}"))
    };
    assert_eq!(outcome(&run_a), Outcome::Stopped);
    assert_eq!(outcome(&run_a_old), Outcome::NotRunning);
    assert!(
        taken.iter().all(|t| t.run_id != run_b),
        "ledger A took over a run of ledger B: {taken:?}"
    );
    assert_eq!(taken.len(), 2, "{taken:?}");
    assert!(old_watchdog::all_gone(&taken));
    assert_eq!(columns(&a, &run_a), (None, None));
    assert_eq!(columns(&a, &run_a_old), (None, None));
    assert_eq!(
        columns(&b, &run_b),
        (Some(i64::from(pid_b)), Some(start_b as i64)),
        "the record of ledger B stays"
    );
    assert!(
        locks.join(format!("watchdog-{run_b}.lock")).exists(),
        "the lock folder of ledger B stays"
    );
}
