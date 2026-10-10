//! Process groups and `procid`. Here, not in `src/procid.rs`: the CMP-16
//! audit allows a new process group only in named product files.
#![cfg(any(target_os = "macos", target_os = "linux"))]

use horch_core::procid;

/// A group is the same while its leader is, and not once the leader
/// is another process or the group has gone.
#[test]
fn a_group_is_the_same_only_while_its_leader_is() {
    use std::os::unix::process::CommandExt;
    let mut leader = std::process::Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    let pgid = leader.id();
    let started = procid::start_time(pgid).unwrap();
    assert!(procid::is_same_group(pgid, started));
    assert!(!procid::is_same_group(pgid, started + 1), "a later leader");
    // This process runs, but leads no group of that id.
    let me = std::process::id();
    let mine = procid::start_time(me).unwrap();
    if unsafe { libc::getpgid(0) } != me as i32 {
        assert!(!procid::is_same_group(me, mine), "not a group leader");
    }
    leader.kill().unwrap();
    leader.wait().unwrap();
    assert!(!procid::is_same_group(pgid, started), "the group has gone");
}

/// The leader has ended but its child keeps the group: the id cannot
/// be given out again, so the group is still ours.
#[test]
fn a_group_outlives_its_leader_while_it_has_members() {
    use std::os::unix::process::CommandExt;
    let mut leader = std::process::Command::new("/bin/sh")
        .args(["-c", "sleep 30 </dev/null >/dev/null 2>&1 & exit 0"])
        .process_group(0)
        .spawn()
        .unwrap();
    let pgid = leader.id();
    let started = procid::start_time(pgid).unwrap_or(0);
    leader.wait().unwrap();
    assert!(
        procid::is_same_group(pgid, started),
        "the sleep keeps the group"
    );
    // SAFETY: the group of this test's own `sleep`.
    unsafe { libc::kill(-(pgid as i32), libc::SIGKILL) };
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while procid::is_same_group(pgid, started) && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!procid::is_same_group(pgid, started), "the group has gone");
}
