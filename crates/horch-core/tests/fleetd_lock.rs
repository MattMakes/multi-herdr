//! The fleetd lock and the generation of the `fleetd` row: 1 fleetd for
//! each ledger (docs/specs/fleetd/design.md, "One fleetd for each ledger";
//! docs/tickets/fleetd-p1/14-fleetd-lock-generation.md).
//!
//! A test that needs 2 processes starts this test binary again as a child
//! with `FLEETD_LOCK_CHILD` set. The child runs only [`fleetd_child`], which
//! does the start of a fleetd and reports on stdout.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::fleetd_row;
use horch_core::fleetd::fleet_runtime::application::ledger::DoorError;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::taskledger::store::Store;
use tempfile::TempDir;

/// The env flag that makes this binary a child fleetd.
const CHILD: &str = "FLEETD_LOCK_CHILD";
/// The env key of the ledger of a child.
const CHILD_DB: &str = "FLEETD_LOCK_DB";
/// The env key of the fleet root of a child.
const CHILD_ROOT: &str = "FLEETD_LOCK_ROOT";
/// The stdin line that tells a child to do 1 guarded write.
const WRITE: &str = "write";
/// The mark before each report of a child.
const REPORT: &str = "FLEETD ";
/// The reports of a child.
const BUSY: &str = "busy";
const STARTED: &str = "started";
const WROTE: &str = "wrote";
const LOST: &str = "lost";
/// The longest wait for 1 report line or for the exit of a child.
const CHILD_WAIT: Duration = Duration::from_secs(30);

/// The `fleetd` row.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    generation: i64,
    pid: i64,
    pid_start: i64,
    fleet_clock_s: i64,
}

/// The report of a child that started with generation `g`.
fn started(g: i64) -> String {
    format!("{STARTED} {g}")
}

/// A fleet in temporary folders: its ledger and its fleet root.
struct Fleet {
    dir: TempDir,
}

impl Fleet {
    fn new() -> Fleet {
        Fleet {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("ledger.sqlite3")
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("fleet")
    }

    fn store(&self) -> Store {
        Store::open(&self.db()).unwrap()
    }

    /// The `fleetd` row.
    fn row(&self) -> Option<Row> {
        let store = self.store();
        let mut stmt = store
            .conn()
            .prepare("SELECT generation, pid, pid_start, fleet_clock_s FROM fleetd WHERE id = 1")
            .unwrap();
        let mut rows = stmt
            .query_map([], |r| {
                Ok(Row {
                    generation: r.get(0)?,
                    pid: r.get(1)?,
                    pid_start: r.get(2)?,
                    fleet_clock_s: r.get(3)?,
                })
            })
            .unwrap();
        rows.next().map(Result::unwrap)
    }

    /// Start a child fleetd of this fleet.
    fn child(&self) -> Fleetd {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "fleetd_child", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .env(CHILD_DB, self.db())
            .env(CHILD_ROOT, self.root())
            .env("HORCH_LEDGER_DB", self.db())
            .env("HORCH_STATE_DIR", self.dir.path().join("state"))
            .env("HORCH_DATA_DIR", self.dir.path().join("data"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                // libtest can print its test name first on the same line.
                if let Some((_, report)) = line.split_once(REPORT) {
                    if tx.send(report.to_string()).is_err() {
                        return;
                    }
                }
            }
        });
        Fleetd {
            pid: i64::from(child.id()),
            stdin: child.stdin.take(),
            child: Some(child),
            reports: rx,
        }
    }
}

/// A child fleetd and its report lines. A drop kills a child that still
/// runs.
struct Fleetd {
    pid: i64,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    reports: Receiver<String>,
}

impl Fleetd {
    /// The next report line of the child.
    fn report(&self) -> String {
        self.reports
            .recv_timeout(CHILD_WAIT)
            .expect("the child fleetd reports in time")
    }

    /// Tell the child to do 1 guarded write.
    fn write(&mut self) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{WRITE}").unwrap();
        stdin.flush().unwrap();
    }

    /// The exit code of the child, which ends by itself: its stdin stays
    /// open.
    fn exit_code(mut self) -> i32 {
        let mut child = self.child.take().unwrap();
        let deadline = Instant::now() + CHILD_WAIT;
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return status.code().unwrap();
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("the child fleetd did not exit by itself");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Kill the child and wait until it ends.
    fn kill(mut self) {
        let mut child = self.child.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

impl Drop for Fleetd {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The child fleetd: a no-op unless [`CHILD`] is set. It starts as fleetd
/// does and reports [`BUSY`] or [`STARTED`] with its generation. Then each
/// [`WRITE`] line on stdin does 1 write guarded by its generation and
/// reports [`WROTE`], or reports [`LOST`] and exits, as fleetd does. It also
/// ends at the end of stdin.
#[test]
fn fleetd_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let db = PathBuf::from(std::env::var_os(CHILD_DB).unwrap());
    let root = PathBuf::from(std::env::var_os(CHILD_ROOT).unwrap());
    let mut store = Store::open(&db).unwrap();
    let Some(started) = fleetd_row::start(&root, &mut store).unwrap() else {
        println!("{REPORT}{BUSY}");
        return;
    };
    println!("{REPORT}{}", self::started(started.generation.0));
    for line in std::io::stdin().lock().lines() {
        if line.unwrap() == WRITE {
            let report = guarded_write(&mut store, started.generation);
            println!("{REPORT}{report}");
            if report == LOST {
                return;
            }
        }
    }
}

/// 1 write as the pass does it: check the generation first in the
/// transaction, then write. [`LOST`] when the check fails.
fn guarded_write(store: &mut Store, held: Generation) -> &'static str {
    store
        .immediate(|tx| {
            match fleetd_row::check_generation(tx, held) {
                Err(DoorError::GenerationLost { .. }) => return Ok(LOST),
                other => other?,
            }
            tx.execute("UPDATE fleetd SET fleet_clock_s = 99 WHERE id = 1", [])?;
            Ok(WROTE)
        })
        .unwrap()
}

#[test]
fn first_start_writes_generation_1_and_a_fleet_clock_of_0() {
    let fleet = Fleet::new();
    assert_eq!(fleet.row(), None);
    let mut store = fleet.store();

    let started = fleetd_row::start(&fleet.root(), &mut store)
        .unwrap()
        .expect("the lock is free");

    assert_eq!(started.generation, Generation(1));
    let own_start = horch_platform::process::start_time(std::process::id()).unwrap();
    assert_eq!(
        fleet.row().unwrap(),
        Row {
            generation: 1,
            pid: i64::from(std::process::id()),
            pid_start: i64::try_from(own_start).unwrap(),
            fleet_clock_s: 0,
        }
    );
    assert!(fleetd_row::lock_path(&fleet.root()).is_file());
}

#[test]
fn each_start_adds_1_to_the_generation() {
    let fleet = Fleet::new();
    let mut store = fleet.store();
    for want in 1..=3 {
        let started = fleetd_row::start(&fleet.root(), &mut store)
            .unwrap()
            .expect("the lock is free");
        assert_eq!(started.generation, Generation(want));
        assert_eq!(fleet.row().unwrap().generation, want);
        // A start keeps the fleet clock of the row.
        store
            .conn()
            .execute("UPDATE fleetd SET fleet_clock_s = fleet_clock_s + 7", [])
            .unwrap();
        drop(started);
    }
    assert_eq!(fleet.row().unwrap().fleet_clock_s, 21);
}

#[test]
fn second_fleetd_exits_at_once_while_the_lock_is_held() {
    let fleet = Fleet::new();
    let first = fleet.child();
    assert_eq!(first.report(), started(1));
    let before = fleet.row().unwrap();
    assert_eq!(before.pid, first.pid);

    let t0 = Instant::now();
    let second = fleet.child();
    assert_eq!(second.report(), BUSY);
    assert_eq!(second.exit_code(), 0, "a busy fleetd exits with code 0");
    assert!(t0.elapsed() < Duration::from_secs(10), "it exits at once");

    assert_eq!(fleet.row().unwrap(), before);
}

#[test]
fn second_fleetd_exits_on_a_generation_mismatch() {
    let fleet = Fleet::new();
    let mut old = fleet.child();
    assert_eq!(old.report(), started(1));
    old.write();
    assert_eq!(old.report(), WROTE, "the holder of the generation writes");
    let mut store = fleet.store();
    store
        .conn()
        .execute("UPDATE fleetd SET fleet_clock_s = 0", [])
        .unwrap();

    // The lock file goes away under the old fleetd, so a new fleetd takes a
    // new lock while the old one still runs. Only the generation stops it.
    std::fs::remove_file(fleetd_row::lock_path(&fleet.root())).unwrap();
    let new = fleetd_row::start(&fleet.root(), &mut store)
        .unwrap()
        .expect("the new lock file is free");
    assert_eq!(new.generation, Generation(2));

    old.write();
    assert_eq!(old.report(), LOST);
    assert_eq!(old.exit_code(), 0, "the old fleetd exits by itself");
    let row = fleet.row().unwrap();
    assert_eq!(
        (row.generation, row.fleet_clock_s),
        (2, 0),
        "the old generation wrote nothing"
    );
    assert_eq!(row.pid, i64::from(std::process::id()));

    let lost = store.immediate(|tx| Ok(fleetd_row::check_generation(tx, Generation(1))));
    assert!(matches!(
        lost.unwrap(),
        Err(DoorError::GenerationLost {
            held: Generation(1),
            found: Some(Generation(2)),
        })
    ));
}

#[test]
fn lock_is_free_after_the_holder_is_killed_and_a_new_start_takes_it() {
    let fleet = Fleet::new();
    let lock = fleetd_row::lock_path(&fleet.root());
    let holder = fleet.child();
    assert_eq!(holder.report(), started(1));

    let held_until = Instant::now() + Duration::from_millis(300);
    assert!(!fleetd_row::wait_until_free(&lock, held_until).unwrap());
    assert!(Instant::now() >= held_until, "it waits until the deadline");

    holder.kill();
    let deadline = Instant::now() + Duration::from_secs(10);
    assert!(fleetd_row::wait_until_free(&lock, deadline).unwrap());

    let next = fleet.child();
    assert_eq!(next.report(), started(2));
    let row = fleet.row().unwrap();
    assert_eq!((row.generation, row.pid), (2, next.pid));
}
