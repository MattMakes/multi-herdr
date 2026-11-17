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

use std::fs::File;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use horch_core::procid;
use horch_core::taskledger::{watchdog, Store};
use horch_e2e::harness::{wait_for, Harness};
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

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
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

/// The fleetd processes of the test. Drop stops each one, and the fleetd
/// that the `fleetd` row names.
struct Fleetds {
    db: PathBuf,
    children: Vec<Child>,
}

impl Fleetds {
    /// The PID and start time of the fleetd that the `fleetd` row names.
    fn row(&self) -> Option<(u32, u64)> {
        let store = Store::open(&self.db).ok()?;
        let (pid, started): (i64, i64) = store
            .conn()
            .query_row("SELECT pid, pid_start FROM fleetd WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .ok()?;
        Some((u32::try_from(pid).ok()?, u64::try_from(started).ok()?))
    }

    /// Start `horch fleetd` in the sealed environment of `h`, and wait
    /// until the `fleetd` row names it.
    fn start(&mut self, h: &Harness) -> u32 {
        let out = h.root.join("fleetd.stdout");
        let err = h.root.join("fleetd.stderr");
        let mut cmd = Command::new(Harness::horch_bin());
        h.seal(&mut cmd);
        let child = cmd
            .arg("fleetd")
            .stdin(Stdio::null())
            .stdout(File::create(&out).unwrap())
            .stderr(File::create(&err).unwrap())
            .spawn()
            .unwrap();
        let pid = child.id();
        self.children.push(child);
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            if self.row().is_some_and(|(p, _)| p == pid) {
                return pid;
            }
            assert!(
                Instant::now() < until,
                "horch fleetd (PID {pid}) does not own the fleetd row after 30 s\nstderr: {}",
                std::fs::read_to_string(&err).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Fleetds {
    fn drop(&mut self) {
        if let Some((pid, started)) = self.row() {
            procid::signal_same(pid, started, SIGKILL);
        }
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Record `p` as the old watchdog of `run_id`: the owner of its lock, as
/// `watchdog::lock` writes it, and the columns of the run, as
/// `watchdog::record_start` writes them.
fn record_old_watchdog(h: &Harness, run_id: &str, p: &Proc) -> PathBuf {
    let locks = h.root.join("locks");
    let guard = watchdog::lock(&locks, run_id)
        .unwrap()
        .expect("the watchdog lock");
    let owner_file = guard.path().join("owner");
    let mut owner: Value = serde_json::from_slice(&std::fs::read(&owner_file).unwrap()).unwrap();
    owner["pid"] = p.pid.into();
    owner["started"] = p.started.into();
    std::fs::write(&owner_file, owner.to_string()).unwrap();
    let lock = guard.path().to_path_buf();
    // The lock stays: its owner is the stand-in, not this test.
    std::mem::forget(guard);
    let store = Store::open(&db(h)).unwrap();
    watchdog::record_start(store.conn(), run_id, p.pid, Some(p.started)).unwrap();
    let holder = watchdog::holder(&locks, run_id).expect("the stand-in holds the lock");
    assert_eq!((holder.pid, holder.started), (p.pid, Some(p.started)));
    lock
}

#[test]
fn update_with_a_live_worker_and_a_live_old_watchdog_leaves_1_control_path() {
    let h = world("p1-release");
    // Phase P: a run with 1 claimed task and a live fake worker. Many stall
    // rounds keep a check from reaping the live worker.
    ok(
        &h,
        &[
            "ledger",
            "run",
            "new",
            "zeta",
            "--check-every",
            "1",
            "--stall-rounds",
            "1000",
        ],
    );
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join("t0.md");
    std::fs::write(&ticket, "# T0\n").unwrap();
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &ticket.to_string_lossy(),
            "--primary",
            "opus",
        ],
    );
    let w = ok(&h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    let agent = Proc::sleep();
    horch_core::taskledger::supervisor::agent_started(&db(&h), &w, agent.pid).unwrap();
    let mut claim = h.horch(&["ledger", "claim"]);
    claim.env("HORCH_WORKER_ID", &w);
    let o = claim.output().unwrap();
    assert!(o.status.success(), "horch ledger claim: {}", text(&o));
    let run_id = texts(&h, "SELECT run_id FROM runs").remove(0);
    // The old watchdog of the run is live.
    let old = Proc::sleep();
    let lock = record_old_watchdog(&h, &run_id, &old);

    // The update to Phase 1: fleetd starts.
    let mut fleetds = Fleetds {
        db: db(&h),
        children: Vec::new(),
    };
    let fleetd = fleetds.start(&h);

    // fleetd stops the old watchdog by its PID and start time.
    wait_for("the stop of the old watchdog", || {
        (!old.alive()).then_some(())
    });
    let recorded = texts(
        &h,
        "SELECT run_id FROM runs WHERE watchdog_pid IS NOT NULL OR watchdog_started IS NOT NULL",
    );
    assert!(
        recorded.is_empty(),
        "the record of the old watchdog is cleared"
    );
    assert!(!lock.exists(), "the lock of the old watchdog is removed");
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
