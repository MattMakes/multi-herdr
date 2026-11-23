use horch_platform::process;
use std::process::{Child, Command};
use std::time::SystemTime;

fn short_child() -> Child {
    #[cfg(unix)]
    let child = Command::new("true").spawn();
    #[cfg(windows)]
    let child = Command::new("cmd").args(["/C", "exit 0"]).spawn();
    child.expect("start a child")
}

fn reaped_pid() -> u32 {
    let mut child = short_child();
    let pid = child.id();
    child.wait().expect("wait for the child");
    pid
}

#[test]
fn start_time_of_this_process_is_stable() {
    let me = std::process::id();
    let started = process::start_time(me).expect("this process runs");
    assert_eq!(process::start_time(me), Some(started));
    assert!(process::started_at(me).expect("this process runs") <= SystemTime::now());
}

#[test]
fn start_time_of_a_dead_pid_is_none() {
    let pid = reaped_pid();
    assert_eq!(process::start_time(pid), None);
    assert_eq!(process::started_at(pid), None);
    assert_eq!(process::start_time(0), None);
    assert_eq!(
        process::start_time(u32::MAX),
        None,
        "would be -1: every process"
    );
}

#[test]
fn alive_is_true_for_self_and_false_for_a_reaped_child() {
    assert!(process::alive(std::process::id()));
    assert!(!process::alive(reaped_pid()));
    assert!(!process::alive(0));
    assert!(!process::alive(u32::MAX));
}

#[test]
fn started_by_now_is_true_for_self() {
    let me = std::process::id();
    assert!(process::started_by(me, SystemTime::now()));
    assert!(!process::started_by(me, std::time::UNIX_EPOCH));
}

#[cfg(unix)]
#[test]
fn parent_of_a_child_is_this_process() {
    let mut child = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("start sleep");
    let parent = process::parent(child.id());
    child.kill().expect("stop sleep");
    child.wait().expect("wait for sleep");
    assert_eq!(parent, Ok(Some(std::process::id())));
}

#[cfg(unix)]
#[test]
fn parent_of_a_root_process_is_read() {
    assert_eq!(process::parent(1).unwrap(), Some(0));
}

#[cfg(unix)]
#[test]
fn group_members_hold_this_process() {
    // SAFETY: `getpgrp` reads the group of this process and cannot fail.
    let group = unsafe { libc::getpgrp() } as u32;
    let members = process::group_members(group).expect("supported on unix");
    assert!(members.contains(&std::process::id()), "{members:?}");
}

#[cfg(windows)]
#[test]
fn parent_is_not_supported_on_windows() {
    let err = process::parent(std::process::id()).unwrap_err();
    assert_eq!(err.concern, "process");
    assert_eq!(err.os, "windows");
}

#[cfg(windows)]
#[test]
fn group_members_are_not_supported_on_windows() {
    let err = process::group_members(std::process::id()).unwrap_err();
    assert_eq!(err.concern, "process");
    assert_eq!(err.os, "windows");
}

#[cfg(windows)]
#[test]
fn filetime_counts_from_1601() {
    use std::time::{Duration, UNIX_EPOCH};
    assert_eq!(
        process::filetime_to_system(116_444_736_000_000_000),
        UNIX_EPOCH
    );
    assert_eq!(
        process::filetime_to_system(116_444_736_000_000_000 + 10_000_015),
        UNIX_EPOCH + Duration::new(1, 1_500)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn start_time_parses_a_stat_line_with_a_name_with_spaces() {
    let stat = "42 (a b) c) S 1 42 42 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 98765 0 0";
    assert_eq!(process::parse_stat(stat), Some(98765));
    assert_eq!(process::stat_field::<i32>(stat, 5), Some(42), "the group");
    assert_eq!(process::stat_field::<u32>(stat, 4), Some(1), "the parent");
}
