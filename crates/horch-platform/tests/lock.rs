//! The fleetd lock: 1 process holds it, and the OS frees it when that
//! process ends (design, "One fleetd for each ledger"; FM-12).
//!
//! A test that needs a second process starts this test binary again as a
//! child. The env var `HOLDER_ENV` makes the test `holder` take the lock,
//! print `HELD`, and keep the lock until its stdin closes.

use horch_platform::lock::FleetdLock;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const HOLDER_ENV: &str = "HORCH_PLATFORM_TEST_LOCK_HOLDER";
/// When set, the holder also starts a waiter child that lives on.
const WITH_CHILD_ENV: &str = "HORCH_PLATFORM_TEST_LOCK_WITH_CHILD";
const WAITER_ENV: &str = "HORCH_PLATFORM_TEST_LOCK_WAITER";
const HELD: &str = "HORCH-PLATFORM-TEST-HELD";
const WAITING: &str = "HORCH-PLATFORM-TEST-WAITING";

/// The child mode: take the lock at the path in `HOLDER_ENV`, and hold it
/// until stdin closes. Not a test of its own: it returns at once when the
/// env var is not set.
#[test]
fn holder() {
    let Some(path) = std::env::var_os(HOLDER_ENV) else {
        return;
    };
    let lock = FleetdLock::try_take(Path::new(&path))
        .expect("take the lock")
        .expect("the lock is free");
    // The waiter shares this stdin and stdout, so it lives until the test
    // closes the stdin pipe.
    let waiter = std::env::var_os(WITH_CHILD_ENV).map(|_| {
        test_binary("waiter")
            .env(WAITER_ENV, "1")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .spawn()
            .expect("start the waiter")
    });
    println!("\n{HELD}");
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
    drop(lock);
    if let Some(mut waiter) = waiter {
        let _ = waiter.wait();
    }
}

/// The child mode of the child of a holder: print `WAITING`, then wait
/// until stdin closes.
#[test]
fn waiter() {
    if std::env::var_os(WAITER_ENV).is_none() {
        return;
    }
    println!("\n{WAITING}");
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
}

fn test_binary(test: &str) -> Command {
    let mut cmd = Command::new(std::env::current_exe().expect("test binary"));
    cmd.args(["--exact", test, "--nocapture", "--test-threads=1"]);
    cmd.env_remove(HOLDER_ENV)
        .env_remove(WITH_CHILD_ENV)
        .env_remove(WAITER_ENV);
    cmd
}

/// Start a holder of the lock at `path` and wait until it prints each of
/// `markers`. The caller keeps the stdin pipe of the child open.
fn start_holder(path: &Path, with_child: bool, markers: &[&str]) -> Child {
    let mut cmd = test_binary("holder");
    cmd.env(HOLDER_ENV, path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if with_child {
        cmd.env(WITH_CHILD_ENV, "1");
    }
    let mut child = cmd.spawn().expect("start the holder");
    let stdout = child.stdout.take().expect("stdout pipe");
    wait_for(stdout, markers);
    child
}

/// Read `stdout` until each marker was a whole line. The reader then goes
/// on in a thread, so the children never block on a full pipe.
fn wait_for(stdout: ChildStdout, markers: &[&str]) {
    let mut lines = BufReader::new(stdout).lines();
    let mut missing: Vec<&str> = markers.to_vec();
    while !missing.is_empty() {
        let line = lines
            .next()
            .expect("the child ended before it printed its markers")
            .expect("read stdout");
        missing.retain(|m| line.trim() != *m);
    }
    std::thread::spawn(move || for _ in lines {});
}

/// Try to take the lock until it is free. Windows frees the lock of an
/// ended process a short time after the process ends, so a test waits for
/// it.
fn take_within(path: &Path, limit: Duration) -> Option<FleetdLock> {
    let end = Instant::now() + limit;
    loop {
        if let Some(lock) = FleetdLock::try_take(path).expect("try the lock") {
            return Some(lock);
        }
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn second_take_in_another_process_gets_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleetd.lock");
    let mut holder = start_holder(&path, false, &[HELD]);

    assert!(FleetdLock::try_take(&path).unwrap().is_none());

    drop(holder.stdin.take());
    assert!(holder.wait().unwrap().success());
    assert!(take_within(&path, Duration::from_secs(10)).is_some());
}

#[test]
fn second_take_in_the_same_process_gets_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleetd.lock");
    let first = FleetdLock::try_take(&path).unwrap().expect("free");

    assert!(FleetdLock::try_take(&path).unwrap().is_none());
    drop(first);
}

#[test]
fn lock_is_free_after_the_holder_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleetd.lock");
    let mut holder = start_holder(&path, false, &[HELD]);
    assert!(FleetdLock::try_take(&path).unwrap().is_none());

    // SIGKILL on unix and TerminateProcess on Windows: the holder runs no
    // code of its own after this.
    holder.kill().unwrap();
    holder.wait().unwrap();

    assert!(take_within(&path, Duration::from_secs(10)).is_some());
}

/// A process that the holder starts does not get the lock: the lock is
/// free when the holder is killed, while its child still runs.
#[test]
fn lock_is_free_after_the_holder_is_killed_while_its_child_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleetd.lock");
    let mut holder = start_holder(&path, true, &[HELD, WAITING]);
    assert!(FleetdLock::try_take(&path).unwrap().is_none());

    holder.kill().unwrap();
    holder.wait().unwrap();
    let taken = take_within(&path, Duration::from_secs(10));

    // The waiter ends when its stdin, the pipe of this test, closes.
    drop(holder.stdin.take());
    assert!(taken.is_some(), "the child of the holder keeps the lock");
}

#[test]
fn lock_is_free_after_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleetd.lock");
    let first = FleetdLock::try_take(&path).unwrap().expect("free");
    drop(first);

    // Other tests of this binary start children on parallel threads. A
    // child that starts while the lock is held gets a copy of the open lock
    // file, and the copy holds the lock until the exec of the child closes
    // it (close-on-exec). So the second take retries for a bounded time.
    // Measured: 4 of 300 runs failed without the retry, 0 of 1000 runs
    // failed with this test alone.
    assert!(take_within(&path, Duration::from_secs(5)).is_some());
}

#[test]
fn take_creates_the_lock_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleetd.lock");
    let _lock = FleetdLock::try_take(&path).unwrap().expect("free");

    assert!(path.is_file());
}

#[test]
fn take_in_a_missing_folder_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run").join("fleetd.lock");

    let err = FleetdLock::try_take(&path).expect_err("an error");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}
