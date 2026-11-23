use horch_platform::signal as platform_signal;

pub fn start_time(pid: u32) -> Option<u64> {
    horch_platform::process::start_time(pid)
}

pub fn started_at(pid: u32) -> Option<std::time::SystemTime> {
    horch_platform::process::started_at(pid)
}

pub fn alive(pid: u32, started: Option<u64>) -> bool {
    if !crate::fsx::pid_alive(pid) {
        return false;
    }
    match (started, start_time(pid)) {
        (Some(want), Some(now)) => want == now,
        _ => true,
    }
}

pub fn is_same(pid: u32, started: u64) -> bool {
    start_time(pid) == Some(started)
}

pub fn is_same_sandboxed(pid: u32, started: u64) -> bool {
    horch_platform::process::start_time_sandboxed(pid) == Some(started)
}

pub fn parent_sandboxed(pid: u32) -> Option<u32> {
    horch_platform::process::parent_sandboxed(pid)
}

pub fn is_same_group(pgid: u32, started: u64) -> bool {
    horch_platform::group::is_same_group(pgid, started).unwrap_or(false)
}

pub fn group_members(pgid: u32) -> Vec<u32> {
    horch_platform::process::group_members(pgid).unwrap_or_default()
}

pub fn started_by(pid: u32, t: std::time::SystemTime) -> bool {
    horch_platform::process::started_by(pid, t)
}

pub fn os_pid(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|p| *p > 0)
}

pub fn signal_same(pid: u32, started: u64, signal: i32) -> bool {
    platform_signal::signal_same(pid, started, signal).unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Term,
    Kill,
}

pub fn stop_same(pid: u32, started: u64, how: Stop) -> bool {
    let how = match how {
        Stop::Term => platform_signal::Stop::Term,
        Stop::Kill => platform_signal::Stop::Kill,
    };
    platform_signal::stop_same(pid, started, how)
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

    #[cfg(target_os = "macos")]
    #[test]
    fn the_sandbox_safe_sysctl_names_the_same_process_start() {
        let me = std::process::id();
        assert_eq!(
            horch_platform::process::start_time_sandboxed(me),
            start_time(me),
            "the sandbox fallback keeps the recorded identity"
        );
        assert!(is_same_sandboxed(me, start_time(me).unwrap()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_sandbox_safe_sysctl_names_the_parent() {
        // SAFETY: `getppid` has no preconditions.
        let parent = unsafe { libc::getppid() };
        assert_eq!(
            parent_sandboxed(std::process::id()),
            u32::try_from(parent).ok()
        );
        assert_eq!(parent_sandboxed(i32::MAX as u32), None, "no such process");
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
        assert!(
            theirs + Duration::from_secs(2) >= before,
            "{theirs:?} {before:?}"
        );
        assert!(theirs >= mine);
    }

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
