//! Phase 0 red tests for the supervision of fleetd (`docs/specs/fleetd/design.md`,
//! "Supervision, liveness and reconciliation", FM-7 and FM-34). Both turn
//! green in Phase 1.
//!
//! The tests stand in for the service manager: they call `horch fleetd check`
//! themselves, as the 30 s timer of the design does. They read the PID of
//! fleetd from the `fleetd` row of the ledger of the harness.

#![cfg(unix)]

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime};

use horch_core::procid;
use horch_core::taskledger::Store;
use horch_e2e::harness::Harness;
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

fn json_of(h: &Harness, args: &[&str]) -> Value {
    let o = h.run(args);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

fn ticket(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.md"));
    std::fs::write(&p, body).unwrap();
    p
}

/// A harness with its own ledger file and a git repository.
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

/// The PID and start time of the fleetd that the `fleetd` row of the ledger
/// names (design, "Ledger"). `None` while no row exists.
fn fleetd_row(db: &Path) -> Option<(u32, Option<u64>)> {
    let store = Store::open(db).ok()?;
    let (pid, started): (i64, String) = store
        .conn()
        .query_row("SELECT pid, pid_start FROM fleetd WHERE id = 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok()?;
    Some((u32::try_from(pid).ok()?, started.parse().ok()))
}

/// The status and the attempt count of task 1.
fn task_row(db: &Path) -> (String, i64) {
    Store::open(db)
        .unwrap()
        .conn()
        .query_row(
            "SELECT status, attempts FROM tasks WHERE seq = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

/// Stop the process `pid` only while it is the one that started at `started`.
fn stop(pid: u32, started: Option<u64>) {
    if let Some(started) = started.or_else(|| procid::start_time(pid)) {
        procid::signal_same(pid, started, SIGKILL);
    }
}

/// Each fleetd of a test: the processes that the test started, and the one
/// that the `fleetd` row names. Drop stops them all, also when the test
/// fails.
struct Fleetds {
    db: PathBuf,
    children: Vec<Child>,
    seen: Vec<(u32, Option<u64>)>,
}

impl Fleetds {
    fn new(h: &Harness) -> Self {
        Fleetds {
            db: db(h),
            children: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// The fleetd that the `fleetd` row names, recorded for the teardown.
    fn current(&mut self) -> Option<(u32, Option<u64>)> {
        let row = fleetd_row(&self.db)?;
        if !self.seen.contains(&row) {
            self.seen.push(row);
        }
        Some(row)
    }
}

impl Drop for Fleetds {
    fn drop(&mut self) {
        let _ = self.current();
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        for &(pid, started) in &self.seen {
            stop(pid, started);
        }
    }
}

/// Start `<bin> fleetd` in the sealed environment of `h`, and wait until the
/// `fleetd` row names it. Returns its PID.
fn start_fleetd(h: &Harness, fleetds: &mut Fleetds, bin: &Path) -> u32 {
    let out = h.root.join("fleetd.stdout");
    let err = h.root.join("fleetd.stderr");
    let mut cmd = Command::new(bin);
    h.seal(&mut cmd);
    let child = cmd
        .arg("fleetd")
        .stdin(Stdio::null())
        .stdout(File::create(&out).unwrap())
        .stderr(File::create(&err).unwrap())
        .spawn()
        .unwrap();
    let pid = child.id();
    fleetds.children.push(child);
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        if fleetds.current().is_some_and(|(p, _)| p == pid) {
            return pid;
        }
        let child = fleetds.children.last_mut().unwrap();
        if let Some(status) = child.try_wait().unwrap() {
            panic!(
                "horch fleetd exited at start: status: {status}\nstdout: {}\nstderr: {}",
                std::fs::read_to_string(&out).unwrap_or_default(),
                std::fs::read_to_string(&err).unwrap_or_default()
            );
        }
        assert!(
            Instant::now() < until,
            "horch fleetd (PID {pid}) runs, but after 30 s the fleetd row of the ledger names {:?}",
            fleetd_row(&fleetds.db)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Kills the stand-in agent and the watchdog that holds a lock in `locks`
/// when dropped, so a failed assertion leaks neither.
struct WatchdogTeardown {
    agent: Child,
    locks: PathBuf,
}

impl Drop for WatchdogTeardown {
    fn drop(&mut self) {
        let locks = std::fs::read_dir(&self.locks).into_iter().flatten();
        locks
            .flatten()
            .for_each(|lock| kill_lock_owner(&lock.path()));
        let _ = self.agent.kill();
        let _ = self.agent.wait();
    }
}

/// Kill the process that the `owner` file of `lock` names, if it still runs.
fn kill_lock_owner(lock: &Path) {
    let owner = std::fs::read(lock.join("owner")).unwrap_or_default();
    let v: Value = serde_json::from_slice(&owner).unwrap_or_default();
    if let (Some(pid), Some(started)) = (v["pid"].as_u64(), v["started"].as_u64()) {
        procid::signal_same(pid as u32, started, SIGKILL);
    }
}

/// In `h`, run `zeta` with task 1 claimed by a worker whose agent is a live
/// `sleep` child of the test. Many stall rounds keep a watchdog from reaping
/// the live worker. Returns the output of the worker's `claim`.
fn claimed_run(h: &Harness) -> (WatchdogTeardown, Output) {
    ok(
        h,
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
    let p = ticket(h, "t0", "# T0\n");
    ok(
        h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            "opus",
        ],
    );
    let w = ok(h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    let teardown = WatchdogTeardown {
        agent: Command::new("/bin/sleep").arg("600").spawn().unwrap(),
        locks: h.root.join("locks"),
    };
    horch_core::taskledger::supervisor::agent_started(&db(h), &w, teardown.agent.id()).unwrap();
    let mut cmd = h.horch(&["ledger", "claim"]);
    cmd.env("HORCH_WORKER_ID", &w);
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "horch ledger claim: {}", text(&o));
    (teardown, o)
}

#[test]
fn fm7_killed_watchdog_does_not_stop_the_reconcile() {
    horch_e2e::expected::check(
        file!(),
        "fm7_killed_watchdog_does_not_stop_the_reconcile",
        || {
            let mut h = world("p0-fm7");
            h.unset("HORCH_NO_WATCHDOG");
            let (mut teardown, o) = claimed_run(&h);
            assert!(
                text(&o).contains("the watchdog of run zeta was not running; started it"),
                "{}",
                text(&o)
            );
            let mut fleetds = Fleetds::new(&h);
            let st = json_of(&h, &["ledger", "status", "--json"]);
            let run_id = st["run"]["run_id"]
                .as_str()
                .expect("the run id")
                .to_string();
            let locks = h.root.join("locks");
            let holder = horch_e2e::harness::wait_for("the watchdog's lock", || {
                horch_core::taskledger::watchdog::holder(&locks, &run_id)
            });
            let started = holder.started.expect("the watchdog's start time");
            // From here on, no `horch` call runs other than `horch fleetd check`.
            assert!(procid::signal_same(holder.pid, started, SIGKILL));
            horch_e2e::harness::wait_for("the watchdog to end", || {
                (!procid::alive(holder.pid, Some(started))).then_some(())
            });
            teardown.agent.kill().unwrap();
            teardown.agent.wait().unwrap();
            let died = Instant::now();
            // The timer of the design runs `horch fleetd check` every 30 s.
            let mut checked: Option<Instant> = None;
            loop {
                if checked.is_none_or(|t| t.elapsed() >= Duration::from_secs(30)) {
                    let o = h.run(&["fleetd", "check"]);
                    assert!(o.status.success(), "horch fleetd check: {}", text(&o));
                    checked = Some(Instant::now());
                    let _ = fleetds.current();
                }
                let (status, _) = task_row(&db(&h));
                if status == "queued" {
                    break;
                }
                assert!(
                    died.elapsed() < Duration::from_secs(35),
                    "task 1 is {status}, not queued, 35 s after its agent died"
                );
                std::thread::sleep(Duration::from_millis(250));
            }
        },
    );
}

/// A copy of this build's horch at `path`, put in place by a rename, as
/// `horch self-update` replaces its binary. Its modification time is
/// `mtime`.
fn install_copy(h: &Harness, path: &Path, mtime: SystemTime) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let tmp = h.root.join("horch.download");
    std::fs::copy(Harness::horch_bin(), &tmp).unwrap();
    File::options()
        .write(true)
        .open(&tmp)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    // `--version` exits before any command runs.
    let warmed = horch_e2e::harness::warm(Command::new(&tmp).arg("--version")).unwrap();
    assert!(warmed.status.success(), "warming {} failed", tmp.display());
    std::fs::rename(&tmp, path).unwrap();
}

#[test]
fn fm34_daemon_restart_on_a_new_binary_keeps_the_run() {
    horch_e2e::expected::check(
        file!(),
        "fm34_daemon_restart_on_a_new_binary_keeps_the_run",
        || {
            let h = world("p0-fm34");
            let (_teardown, _) = claimed_run(&h);
            assert_eq!(task_row(&db(&h)), ("running".to_string(), 1));
            let copy = h.root.join("install").join("horch");
            let day = Duration::from_secs(24 * 3600);
            install_copy(&h, &copy, SystemTime::now() - day);
            let mut fleetds = Fleetds::new(&h);
            let old = start_fleetd(&h, &mut fleetds, &copy);
            let replaced = SystemTime::now();
            install_copy(&h, &copy, SystemTime::now());
            // 2 passes of 5 s.
            let until = Instant::now() + Duration::from_secs(10);
            let new = loop {
                if let Some((pid, started)) = fleetds.current() {
                    if pid != old && procid::alive(pid, started) {
                        break pid;
                    }
                }
                assert!(
                    Instant::now() < until,
                    "no new fleetd runs 10 s after its binary changed; the fleetd row names {:?}",
                    fleetd_row(&db(&h))
                );
                std::thread::sleep(Duration::from_millis(100));
            };
            let old_child = fleetds.children.first_mut().unwrap();
            assert!(
                old_child.try_wait().unwrap().is_some(),
                "the old fleetd (PID {old}) still runs beside the new one (PID {new})"
            );
            let started = procid::started_at(new).expect("the new fleetd's start time");
            assert!(
                started + Duration::from_secs(1) >= replaced,
                "the new fleetd (PID {new}) started before the binary changed"
            );
            let command = horch_e2e::process::processes()
                .into_iter()
                .find(|p| p.pid == new)
                .map(|p| p.command)
                .unwrap_or_default();
            assert!(
                command.starts_with(&copy.to_string_lossy().into_owned()),
                "the new fleetd (PID {new}) runs `{command}`, not {}",
                copy.display()
            );
            assert_eq!(
                task_row(&db(&h)),
                ("running".to_string(), 1),
                "the run goes on with 1 attempt for task 1"
            );
        },
    );
}
