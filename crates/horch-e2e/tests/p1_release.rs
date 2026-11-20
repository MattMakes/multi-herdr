//! The release test of Phase 1 (`docs/specs/fleetd/design.md`, "What a
//! release does to a run in flight", row Phase 1;
//! `docs/tickets/fleetd-p1/MAP.md`, Q10 and Q21).
//!
//! A run of Phase P has a live worker and a live old watchdog. After the
//! update to Phase 1, fleetd starts: it stops the old watchdog by its PID
//! and start time and takes its run. 1 control path acts, the worker goes
//! on, and the task has 1 attempt.
//!
//! The old watchdog is a stand-in: a `sleep` child of the test, recorded as
//! the owner of the watchdog lock of the run and in `runs.watchdog_pid`, as
//! `horch ledger watch` recorded itself before Phase 1.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

use horch_core::procid;
use horch_core::taskledger::runs::Run;
use horch_core::taskledger::{watchdog, Store};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

const SIGKILL: i32 = 9;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A harness with its own ledger file and a git repository. No command
/// makes the fleetd check: the test starts fleetd itself.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

fn db(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

/// The first column of each row of `sql`, as text.
fn texts(h: &Harness, sql: &str) -> Vec<String> {
    let store = Store::open(&db(h)).unwrap();
    let mut stmt = store.conn().prepare(sql).unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

/// A process that the test started: killed at the end, also when the test
/// fails.
struct Proc {
    pid: u32,
    started: u64,
}

impl Proc {
    /// A `sleep` child. A thread waits for it, so it leaves no zombie when
    /// it ends.
    fn sleep() -> Proc {
        let mut child = Command::new("/bin/sleep").arg("600").spawn().unwrap();
        let pid = child.id();
        let started = procid::start_time(pid).expect("the start time of sleep");
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Proc { pid, started }
    }

    fn alive(&self) -> bool {
        procid::alive(self.pid, Some(self.started))
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        procid::signal_same(self.pid, self.started, SIGKILL);
    }
}

/// Record `p` as the old watchdog of `run_id`: the owner of its lock, as
/// `watchdog::lock` writes it, and the columns of the run, as
/// `watchdog::record_start` writes them. With `started` `None`, the record
/// has no start time.
fn record_old_watchdog(h: &Harness, run_id: &str, p: &Proc, started: Option<u64>) -> PathBuf {
    let locks = h.root.join("locks");
    let guard = watchdog::lock(&locks, run_id)
        .unwrap()
        .expect("the watchdog lock");
    let owner_file = guard.path().join("owner");
    let mut owner: Value = serde_json::from_slice(&std::fs::read(&owner_file).unwrap()).unwrap();
    owner["pid"] = p.pid.into();
    owner["started"] = started.into();
    std::fs::write(&owner_file, owner.to_string()).unwrap();
    let lock = guard.path().to_path_buf();
    // The lock stays: its owner is the stand-in, not this test.
    std::mem::forget(guard);
    let store = Store::open(&db(h)).unwrap();
    watchdog::record_start(store.conn(), run_id, p.pid, started).unwrap();
    lock
}

fn set_thresholds(h: &Harness, run: &Run, check_every_s: i64, stall_rounds: i64) {
    Store::open(&db(h))
        .unwrap()
        .conn()
        .execute(
            "UPDATE runs SET check_every_s = ?2, stall_rounds = ?3 WHERE run_id = ?1",
            rusqlite::params![run.run_id, check_every_s, stall_rounds],
        )
        .unwrap();
}

/// Phase P in `h`: run `zeta` with 1 bound task and a live fake worker.
/// Many stall rounds keep a check from reaping the live worker. Gives the
/// worker id, its agent and the run id.
fn phase_p_run(h: &Harness) -> (String, Proc, String) {
    let run = seed_run(h, "zeta", None, "w1:p1");
    set_thresholds(h, &run, 1, 1000);
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join("t0.md");
    std::fs::write(&ticket, "# T0\n").unwrap();
    seed_task(h, &run, &ticket, "opus");
    let agent = Proc::sleep();
    let w = horch_e2e::bind::bind(h, "opus", Some(agent.pid)).worker_id;
    let run_id = texts(h, "SELECT run_id FROM runs").remove(0);
    (w, agent, run_id)
}

#[test]
fn update_with_a_live_worker_and_a_live_old_watchdog_leaves_1_control_path() {
    let h = world("p1-release");
    let (w, agent, run_id) = phase_p_run(&h);
    // The old watchdog of the run is live.
    let old = Proc::sleep();
    let lock = record_old_watchdog(&h, &run_id, &old, Some(old.started));
    let holder =
        watchdog::holder(&h.root.join("locks"), &run_id).expect("the stand-in holds the lock");
    assert_eq!((holder.pid, holder.started), (old.pid, Some(old.started)));

    // The update to Phase 1: fleetd starts.
    let mut fleetds = Fleetds::new(&h);
    let fleetd = fleetds.start(&h, "fleetd");

    // fleetd stops the old watchdog by its PID and start time. It clears the
    // record in a later transaction, so the wait is for both.
    let recorded_watchdogs = || {
        texts(
            &h,
            "SELECT run_id FROM runs WHERE watchdog_pid IS NOT NULL OR watchdog_started IS NOT NULL",
        )
    };
    wait_for("the stop of the old watchdog", || {
        (!old.alive()).then_some(())
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let remaining = loop {
        let rows = recorded_watchdogs();
        if rows.is_empty() || std::time::Instant::now() >= until {
            break rows;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(
        remaining.is_empty(),
        "the record of the old watchdog is cleared"
    );
    wait_for("the lock of the old watchdog to be removed", || {
        (!lock.exists()).then_some(())
    });
    let events = texts(
        &h,
        "SELECT data_json FROM events WHERE cause = 'old_watchdog_stopped'",
    );
    assert_eq!(events.len(), 1, "{events:?}");
    let data: Value = serde_json::from_str(&events[0]).unwrap();
    assert_eq!(data["pid"], old.pid);
    assert_eq!(data["signalled"], true);

    // Let fleetd run a few passes: 1 control path acts.
    std::thread::sleep(Duration::from_secs(3));
    let (pid, started) = fleetds.row().expect("the fleetd row");
    assert_eq!(pid, fleetd, "the fleetd row names the fleetd of the update");
    assert!(procid::alive(pid, Some(started)), "fleetd runs");
    assert!(
        watchdog::holder(&h.root.join("locks"), &run_id).is_none(),
        "no watchdog runs"
    );
    // The worker goes on, and the task has 1 attempt.
    assert!(agent.alive(), "the worker goes on");
    let worker = texts(
        &h,
        &format!("SELECT status FROM workers WHERE worker_id = '{w}'"),
    );
    assert_eq!(worker, ["working"]);
    let task = texts(
        &h,
        "SELECT status || ' ' || attempts FROM tasks WHERE seq = 1",
    );
    assert_eq!(task, ["running 1"], "the task has 1 attempt");
    let attempts = texts(&h, "SELECT id FROM attempts WHERE task_id IS NOT NULL");
    assert_eq!(attempts.len(), 1, "{attempts:?}");
}

/// 1 control path: while an old watchdog may still run, fleetd runs no
/// pass. The record names a live process but has no start time, so fleetd
/// cannot tell the watchdog from another program with a reused PID.
#[test]
fn fleetd_runs_no_pass_while_an_old_watchdog_may_still_run() {
    let h = world("p1-release-gate");
    let (_, agent, run_id) = phase_p_run(&h);
    let old = Proc::sleep();
    let lock = record_old_watchdog(&h, &run_id, &old, None);
    // A pass would end the attempt of the dead agent and queue its task.
    drop(agent);
    let mut fleetds = Fleetds::new(&h);
    let pid = fleetds.spawn(&h, "fleetd");
    let status = fleetds.wait_exit(pid);
    let err = fleetds.output("fleetd", "stderr");
    assert!(!status.success(), "{err}");
    assert!(err.contains("an old watchdog may still run"), "{err}");
    assert!(old.alive(), "fleetd sends no signal");
    let recorded = texts(&h, "SELECT CAST(watchdog_pid AS TEXT) FROM runs");
    assert_eq!(recorded, [old.pid.to_string()], "the record stays");
    assert!(lock.exists(), "the lock of the old watchdog stays");
    let task = texts(&h, "SELECT status FROM tasks WHERE seq = 1");
    assert_eq!(task, ["running"], "no pass ran");
    // horch ledger watch --once runs no pass either.
    let o = h.run(&["ledger", "watch", "--once"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("an old watchdog may still run"),
        "{}",
        text(&o)
    );
    let task = texts(&h, "SELECT status FROM tasks WHERE seq = 1");
    assert_eq!(task, ["running"], "no pass ran");
}
