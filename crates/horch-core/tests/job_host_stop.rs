#![cfg(unix)]

use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Command;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

const HELPER: &str = "HORCH_JOB_HOST_STOP_HELPER";

fn sleeping_group() -> std::process::Child {
    Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .expect("start a group")
}

#[test]
fn a_sigterm_kills_every_registered_group() {
    let mut first = sleeping_group();
    let mut second = sleeping_group();
    let helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "two_registrations_helper",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(HELPER, format!("{} {}", first.id(), second.id()))
        .output()
        .expect("run the helper");
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until
        && [&mut first, &mut second]
            .into_iter()
            .any(|g| g.try_wait().unwrap().is_none())
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    let alive: Vec<u32> = [&mut first, &mut second]
        .into_iter()
        .filter_map(|g| {
            g.try_wait().unwrap().is_none().then(|| {
                let _ = g.kill();
                g.id()
            })
        })
        .collect();
    let first = first.wait().unwrap();
    let second = second.wait().unwrap();
    assert_eq!(
        helper.status.signal(),
        Some(libc::SIGTERM),
        "the helper ends by SIGTERM: {helper:?}"
    );
    assert!(
        alive.is_empty(),
        "groups alive after the SIGTERM: {alive:?}"
    );
    assert_eq!(
        first.signal(),
        Some(libc::SIGKILL),
        "the 1st group is killed"
    );
    assert_eq!(
        second.signal(),
        Some(libc::SIGKILL),
        "the 2nd group is killed"
    );
}

#[test]
fn two_registrations_helper() {
    static FIRST: AtomicI32 = AtomicI32::new(0);
    static SECOND: AtomicI32 = AtomicI32::new(0);
    let Ok(pgids) = std::env::var(HELPER) else {
        return;
    };
    let mut pgids = pgids.split(' ').map(|p| p.parse::<i32>().unwrap());
    FIRST.store(pgids.next().unwrap(), Ordering::SeqCst);
    SECOND.store(pgids.next().unwrap(), Ordering::SeqCst);
    horch_platform::signal::kill_group_on_termination(&FIRST).expect("supported on unix");
    horch_platform::signal::kill_group_on_termination(&SECOND).expect("supported on unix");
    // SAFETY: the handler kills the groups and then ends this helper process.
    unsafe { libc::raise(libc::SIGTERM) };
    unreachable!("SIGTERM ends the helper");
}
