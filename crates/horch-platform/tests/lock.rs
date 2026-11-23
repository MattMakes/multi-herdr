use horch_platform::lock::FleetdLock;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const HOLDER_ENV: &str = "HORCH_PLATFORM_TEST_LOCK_HOLDER";
const WITH_CHILD_ENV: &str = "HORCH_PLATFORM_TEST_LOCK_WITH_CHILD";
const WAITER_ENV: &str = "HORCH_PLATFORM_TEST_LOCK_WAITER";
const HELD: &str = "HORCH-PLATFORM-TEST-HELD";
const WAITING: &str = "HORCH-PLATFORM-TEST-WAITING";

#[test]
fn holder() {
    let Some(path) = std::env::var_os(HOLDER_ENV) else {
        return;
    };
    let lock = FleetdLock::try_take(Path::new(&path))
        .expect("take the lock")
        .expect("the lock is free");
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

    holder.kill().unwrap();
    holder.wait().unwrap();

    assert!(take_within(&path, Duration::from_secs(10)).is_some());
}

#[test]
fn lock_is_free_after_the_holder_is_killed_while_its_child_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleetd.lock");
    let mut holder = start_holder(&path, true, &[HELD, WAITING]);
    assert!(FleetdLock::try_take(&path).unwrap().is_none());

    holder.kill().unwrap();
    holder.wait().unwrap();
    let taken = take_within(&path, Duration::from_secs(10));

    drop(holder.stdin.take());
    assert!(taken.is_some(), "the child of the holder keeps the lock");
}

#[test]
fn lock_is_free_after_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleetd.lock");
    let first = FleetdLock::try_take(&path).unwrap().expect("free");
    drop(first);

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
