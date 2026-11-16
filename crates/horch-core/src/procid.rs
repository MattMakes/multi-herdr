//! Process identity: a pid with its start time.
//!
//! A pid alone does not name a process. Once a process ends, the OS can give
//! its pid to any later program, so a pid read from a file, a heartbeat or a
//! lock can name a stranger. A signal to that pid ends the wrong program, and
//! a liveness check on it keeps a stale lock alive for ever.
//!
//! So every stored pid is stored with [`start_time`], and every signal and
//! liveness check first confirms that the pid still has that start time.
//! A child the caller still holds unreaped is safe without this: its zombie
//! keeps the pid.
//!
//! The start time is opaque: compare it for equality, never for order across
//! platforms. macOS gives microseconds since the epoch (`proc_pidinfo`),
//! Linux gives clock ticks since boot (`/proc/<pid>/stat` field 22),
//! Windows gives the creation time from `GetProcessTimes` (a `FILETIME`:
//! 100 ns ticks since 1601-01-01). The OS calls are in
//! [`horch_platform::process`] and [`horch_platform::signal`].
//!
//! [`stop_same`] stops a process only while it has its recorded start time.
//! On Unix it sends `SIGTERM` or `SIGKILL` through `signal_same`. Windows
//! has no polite stop for an arbitrary process, so [`Stop::Term`] and
//! [`Stop::Kill`] both call `TerminateProcess` with exit code 1, on the same
//! handle the start time was read through.

use horch_platform::signal as platform_signal;

/// The start time of `pid`, or `None` when it does not run or this platform
/// cannot read it.
pub fn start_time(pid: u32) -> Option<u64> {
    horch_platform::process::start_time(pid)
}

/// When `pid` started, on the wall clock, or `None` when it does not run or
/// this platform cannot read it. For an order with a time this process
/// took. On Linux it is boot time plus clock ticks, so it can be off by up
/// to a second.
pub fn started_at(pid: u32) -> Option<std::time::SystemTime> {
    horch_platform::process::started_at(pid)
}

/// Whether `pid` runs and is the process that started at `started`. With
/// `started` unknown (an old record), or a platform that cannot read start
/// times, this is "the pid exists".
pub fn alive(pid: u32, started: Option<u64>) -> bool {
    if !crate::fsx::pid_alive(pid) {
        return false;
    }
    match (started, start_time(pid)) {
        (Some(want), Some(now)) => want == now,
        _ => true,
    }
}

/// Whether `pid` runs with start time `started`. The check before any
/// signal: an unreadable start time is not a match.
pub fn is_same(pid: u32, started: u64) -> bool {
    start_time(pid) == Some(started)
}

/// Whether process group `pgid` is still the one whose leader started at
/// `started`: the leader runs with that start time and still leads the
/// group; or the leader has ended and the group still has members, which
/// keeps the id from being given out again. False on a platform without
/// process groups (Windows).
pub fn is_same_group(pgid: u32, started: u64) -> bool {
    horch_platform::group::is_same_group(pgid, started).unwrap_or(false)
}

/// The pids in process group `pgid` now: its members, the leader among
/// them while it runs. Empty when the group has none, or on a platform that
/// cannot list them (Windows).
pub fn group_members(pgid: u32) -> Vec<u32> {
    horch_platform::process::group_members(pgid).unwrap_or_default()
}

/// Whether `pid` runs and started at or before `t` on the wall clock. An
/// unknown start time is not a match. Linux compares in clock ticks since
/// boot, not with [`started_at`], so the whole-second boot time cannot move
/// a start before `t`.
pub fn started_by(pid: u32, t: std::time::SystemTime) -> bool {
    horch_platform::process::started_by(pid, t)
}

/// `pid` as the OS type, when it can name one process: 0 and values that
/// would turn negative (a process group, or every process) are refused.
/// Every cast of a stored pid to `pid_t` goes through here.
pub fn os_pid(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|p| *p > 0)
}

/// Send `signal` to `pid` only while it runs with start time `started`, and
/// say whether it was sent. See [`horch_platform::signal::signal_same`]: on
/// Linux through a pidfd, on macOS the check just before `kill`. A platform
/// without signals (Windows) sends nothing.
pub fn signal_same(pid: u32, started: u64, signal: i32) -> bool {
    platform_signal::signal_same(pid, started, signal).unwrap_or(false)
}

/// How hard a stop is. On Unix [`Stop::Term`] is `SIGTERM`, which the
/// process can catch to clean up, and [`Stop::Kill`] is `SIGKILL`. Windows
/// has no polite stop for an arbitrary process: both are `TerminateProcess`
/// with exit code 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Term,
    Kill,
}

/// Stop `pid` only while it runs with start time `started`, and say whether
/// a stop was sent.
///
/// On Unix this is `signal_same` with `SIGTERM` or `SIGKILL`. On Windows
/// one handle is opened with `PROCESS_TERMINATE` and
/// `PROCESS_QUERY_LIMITED_INFORMATION`, the creation time is read through
/// that handle, and only on a match is `TerminateProcess` called on the same
/// handle. The handle names the process that had the pid when it was opened
/// and keeps that pid from being given out, so a reused pid can never be hit
/// between the check and the stop. Both kinds of stop end the process at
/// once there, with exit code 1.
pub fn stop_same(pid: u32, started: u64, how: Stop) -> bool {
    let how = match how {
        Stop::Term => platform_signal::Stop::Term,
        Stop::Kill => platform_signal::Stop::Kill,
    };
    platform_signal::stop_same(pid, started, how)
}

/// Whether `pid` names a process that has not exited, for
/// [`crate::fsx::pid_alive`] on Windows. A process this user may not open
/// exists all the same. Only the Windows build of `fsx` calls it, until
/// `fsx::pid_alive` calls [`horch_platform::process::alive`] itself.
#[allow(dead_code)]
pub(crate) fn windows_running(pid: i32) -> bool {
    u32::try_from(pid).is_ok_and(horch_platform::process::alive)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_pids_have_no_start_time() {
        assert_eq!(start_time(0), None);
        assert_eq!(start_time(u32::MAX), None, "would be -1: every process");
        assert!(!alive(0, None));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn this_process_is_itself() {
        let me = std::process::id();
        let started = start_time(me).expect("this process runs");
        assert_eq!(start_time(me), Some(started), "stable across reads");
        assert!(is_same(me, started));
        assert!(alive(me, Some(started)));
        assert!(alive(me, None), "an old record: the pid exists");
        assert!(!is_same(me, started + 1));
        assert!(!alive(me, Some(started + 1)), "a reused pid is dead");
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_new_child_started_after_this_process() {
        use std::time::{Duration, SystemTime};
        let mine = started_at(std::process::id()).expect("this process runs");
        assert!(mine <= SystemTime::now() + Duration::from_secs(1));
        let before = SystemTime::now();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let theirs = started_at(child.id()).expect("the child runs");
        child.kill().unwrap();
        child.wait().unwrap();
        // Linux counts start times from a boot time in whole seconds, so
        // the child can read up to 1 s plus 1 clock tick early (CI saw
        // 1.003 s). Allow 2 s.
        assert!(
            theirs + Duration::from_secs(2) >= before,
            "{theirs:?} {before:?}"
        );
        assert!(theirs >= mine);
    }

    /// A child that has ended and been reaped: its record names nothing.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn an_ended_child_is_not_the_same() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let started = start_time(pid).expect("the child runs");
        assert!(is_same(pid, started));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!is_same(pid, started));
        assert!(!alive(pid, Some(started)));
    }

    #[test]
    fn os_pid_refuses_what_is_not_one_process() {
        assert_eq!(os_pid(0), None);
        assert_eq!(os_pid(u32::MAX), None, "would be -1: every process");
        assert_eq!(os_pid(i32::MAX as u32 + 1), None, "would be negative");
        assert_eq!(os_pid(42), Some(42));
        assert!(!crate::fsx::pid_alive(u32::MAX));
        assert!(!alive(u32::MAX, None));
    }

    /// The signal goes only to the process with the recorded start time.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn signal_same_signals_only_the_recorded_process() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let started = start_time(pid).unwrap();
        assert!(
            !signal_same(pid, started + 1, libc::SIGKILL),
            "a reused pid"
        );
        assert_eq!(child.try_wait().unwrap(), None, "the child still runs");
        assert!(!signal_same(0, started, libc::SIGKILL));
        assert!(!signal_same(u32::MAX, started, libc::SIGKILL));
        assert!(signal_same(pid, started, libc::SIGKILL));
        assert!(!child.wait().unwrap().success(), "killed");
        assert!(!signal_same(pid, started, libc::SIGKILL), "gone");
    }

    /// This test's own group holds this process and a child it starts.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn group_members_lists_this_group() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        // SAFETY: plain syscall on this process.
        let group = unsafe { libc::getpgrp() } as u32;
        let members = group_members(group);
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(members.contains(&std::process::id()), "{members:?}");
        assert!(members.contains(&child.id()), "{members:?}");
        assert_eq!(group_members(0), Vec::<u32>::new());
        assert_eq!(group_members(u32::MAX), Vec::<u32>::new());
    }

    /// A process started by a time only from its start on, and a gone
    /// process never.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn started_by_orders_a_start_with_a_time() {
        use std::time::{Duration, SystemTime};
        let before = SystemTime::now();
        std::thread::sleep(Duration::from_millis(50));
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        std::thread::sleep(Duration::from_millis(50));
        let after = SystemTime::now();
        assert!(!started_by(pid, before), "started after `before`");
        assert!(started_by(pid, after));
        assert!(started_by(std::process::id(), after));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!started_by(pid, after), "gone");
        assert!(!started_by(0, after));
    }

    /// `stop_same` stops only the process with the recorded start time.
    #[cfg(unix)]
    #[test]
    fn stop_same_stops_only_the_recorded_process() {
        use std::os::unix::process::ExitStatusExt;
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let started = start_time(pid).unwrap();
        assert!(!stop_same(pid, started + 1, Stop::Term), "a reused pid");
        assert!(!stop_same(pid, started + 1, Stop::Kill), "a reused pid");
        assert_eq!(child.try_wait().unwrap(), None, "the child still runs");
        assert!(!stop_same(0, started, Stop::Kill));
        assert!(!stop_same(u32::MAX, started, Stop::Kill));
        assert!(stop_same(pid, started, Stop::Term));
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
        assert!(!stop_same(pid, started, Stop::Kill), "gone");
    }

    #[cfg(windows)]
    fn long_child() -> std::process::Child {
        std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    /// A child has a start time while it runs and none once it has ended,
    /// even while this process still holds a handle to it.
    #[cfg(windows)]
    #[test]
    fn windows_child_identity_ends_with_it() {
        use std::time::{Duration, SystemTime};
        let before = SystemTime::now();
        let mut child = long_child();
        let pid = child.id();
        let started = start_time(pid).expect("the child runs");
        assert_eq!(start_time(pid), Some(started), "stable across reads");
        assert!(is_same(pid, started));
        assert!(!is_same(pid, started + 1));
        assert!(alive(pid, Some(started)));
        assert!(!alive(pid, Some(started + 1)), "a reused pid is dead");
        let at = started_at(pid).expect("the child runs");
        assert!(at + Duration::from_secs(1) >= before, "{at:?} {before:?}");
        assert!(at <= SystemTime::now() + Duration::from_secs(1));
        assert!(started_by(pid, SystemTime::now() + Duration::from_secs(1)));
        assert!(group_members(pid).is_empty());
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(start_time(pid), None, "ended");
        assert!(!is_same(pid, started));
        assert!(!alive(pid, Some(started)));
        assert!(!crate::fsx::pid_alive(pid));
        assert!(!stop_same(pid, started, Stop::Kill), "gone");
    }

    /// `stop_same` terminates only the process with the recorded start time.
    #[cfg(windows)]
    #[test]
    fn windows_stop_same_stops_only_the_recorded_process() {
        let mut child = long_child();
        let pid = child.id();
        let started = start_time(pid).expect("the child runs");
        assert!(crate::fsx::pid_alive(pid));
        assert!(!stop_same(pid, started + 1, Stop::Term), "a reused pid");
        assert!(!stop_same(pid, started + 1, Stop::Kill), "a reused pid");
        assert_eq!(child.try_wait().unwrap(), None, "the child still runs");
        assert!(!stop_same(0, started, Stop::Kill));
        assert!(!stop_same(u32::MAX, started, Stop::Kill));
        assert!(stop_same(pid, started, Stop::Term));
        assert_eq!(child.wait().unwrap().code(), Some(1), "terminated");
        assert!(!stop_same(pid, started, Stop::Kill), "gone");
    }

    #[cfg(windows)]
    #[test]
    fn filetime_counts_from_1601() {
        use std::time::{Duration, UNIX_EPOCH};
        assert_eq!(
            horch_platform::process::filetime_to_system(116_444_736_000_000_000),
            UNIX_EPOCH
        );
        assert_eq!(
            horch_platform::process::filetime_to_system(116_444_736_000_000_000 + 10_000_015),
            UNIX_EPOCH + Duration::new(1, 1_500)
        );
        assert_eq!(
            horch_platform::process::filetime_to_system(116_444_736_000_000_000 - 10_000_000),
            UNIX_EPOCH - Duration::from_secs(1)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stat_field_22_after_a_command_with_spaces() {
        let stat = "42 (a b) c) S 1 42 42 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 98765 0 0";
        assert_eq!(horch_platform::process::parse_stat(stat), Some(98765));
        assert_eq!(
            horch_platform::process::stat_field::<i32>(stat, 5),
            Some(42),
            "the group"
        );
    }
}
