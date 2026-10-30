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
//! 100 ns ticks since 1601-01-01). Other platforms have no start time here:
//! [`start_time`] is `None` and [`alive`] falls back to "the pid exists".
//!
//! [`stop_same`] stops a process only while it has its recorded start time.
//! On Unix it sends `SIGTERM` or `SIGKILL` through `signal_same`. Windows
//! has no polite stop for an arbitrary process, so [`Stop::Term`] and
//! [`Stop::Kill`] both call `TerminateProcess` with exit code 1, on the same
//! handle the start time was read through.

/// The start time of `pid`, or `None` when it does not run or this platform
/// cannot read it.
pub fn start_time(pid: u32) -> Option<u64> {
    let pid = raw(pid)?;
    imp::start_time(pid)
}

/// When `pid` started, on the wall clock, or `None` when it does not run or
/// this platform cannot read it. For an order with a time this process
/// took. On Linux it is boot time plus clock ticks, so it can be off by up
/// to a second.
pub fn started_at(pid: u32) -> Option<std::time::SystemTime> {
    let pid = raw(pid)?;
    imp::started_at(pid)
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
/// keeps the id from being given out again.
#[cfg(unix)]
pub fn is_same_group(pgid: u32, started: u64) -> bool {
    let Some(group) = raw(pgid) else {
        return false;
    };
    match start_time(pgid) {
        // SAFETY: plain syscall; a pid that has gone answers -1.
        Some(now) => now == started && (unsafe { libc::getpgid(group) }) == group,
        // SAFETY: signal 0 checks that the group has a member; nothing is sent.
        None => (unsafe { libc::kill(-group, 0) }) == 0,
    }
}

/// The pids in process group `pgid` now: its members, the leader among
/// them while it runs. Empty when the group has none, or on a platform that
/// cannot list them (Windows).
pub fn group_members(pgid: u32) -> Vec<u32> {
    match raw(pgid) {
        Some(group) => imp::group_members(group),
        None => Vec::new(),
    }
}

/// Whether `pid` runs and started at or before `t` on the wall clock. An
/// unknown start time is not a match. Linux compares in clock ticks since
/// boot, not with [`started_at`], so the whole-second boot time cannot move
/// a start before `t`.
pub fn started_by(pid: u32, t: std::time::SystemTime) -> bool {
    raw(pid).and_then(|pid| imp::started_by(pid, t)) == Some(true)
}

/// `pid` as the OS type, when it can name one process: 0 and values that
/// would turn negative (a process group, or every process) are refused.
/// Every cast of a stored pid to `pid_t` goes through here.
pub fn os_pid(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|p| *p > 0)
}

fn raw(pid: u32) -> Option<i32> {
    os_pid(pid)
}

/// Send `signal` to `pid` only while it runs with start time `started`, and
/// say whether it was sent.
///
/// On Linux a pidfd is opened first and the start time checked after: the
/// pidfd names the process that had the pid when it was opened, and a match
/// after that proves it is the one recorded. The signal then goes to that
/// process even if it ends and its pid is given out in between. A kernel
/// without pidfds (before 5.3) falls back to the check and `kill`. On
/// macOS there is no pidfd for a process that is not a child: the start
/// time is checked just before `kill`, and the pid would have to end and
/// be given out again between the two calls.
#[cfg(unix)]
pub fn signal_same(pid: u32, started: u64, signal: i32) -> bool {
    raw(pid).is_some_and(|os| signal_os(pid, os, started, signal))
}

/// [`signal_same`] once `pid` is known to name one process, as the OS type
/// `os`: the pidfd route on Linux, else the check and `kill`.
#[cfg(unix)]
fn signal_os(pid: u32, os: i32, started: u64, signal: i32) -> bool {
    #[cfg(target_os = "linux")]
    if let Some(sent) = imp::pidfd_signal(os, signal, || is_same(pid, started)) {
        return sent;
    }
    // SAFETY: plain syscall on a pid checked to be positive.
    is_same(pid, started) && unsafe { libc::kill(os, signal) } == 0
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
/// once there, with exit code 1. Other platforms send nothing.
pub fn stop_same(pid: u32, started: u64, how: Stop) -> bool {
    #[cfg(unix)]
    {
        let signal = match how {
            Stop::Term => libc::SIGTERM,
            Stop::Kill => libc::SIGKILL,
        };
        signal_same(pid, started, signal)
    }
    #[cfg(windows)]
    {
        let _ = how;
        raw(pid).is_some_and(|os| imp::terminate_same(os, started))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, started, how);
        false
    }
}

/// Whether `pid` names a process that has not exited, for
/// [`crate::fsx::pid_alive`] on Windows. A process this user may not open
/// exists all the same.
#[cfg(windows)]
pub(crate) fn windows_running(pid: i32) -> bool {
    imp::running(pid)
}

#[cfg(target_os = "macos")]
mod imp {
    pub(super) fn start_time(pid: i32) -> Option<u64> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: the buffer is a zeroed `proc_bsdinfo` of `size` bytes.
        let n = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        (n == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
    }

    /// The start time is microseconds since the epoch.
    pub(super) fn started_at(pid: i32) -> Option<std::time::SystemTime> {
        let micros = start_time(pid)?;
        Some(std::time::UNIX_EPOCH + std::time::Duration::from_micros(micros))
    }

    pub(super) fn started_by(pid: i32, t: std::time::SystemTime) -> Option<bool> {
        Some(started_at(pid)? <= t)
    }

    pub(super) fn group_members(pgid: i32) -> Vec<u32> {
        // Room for more members than a judge job has; a fuller group is cut.
        const CAP: usize = 4096;
        let mut pids = vec![0 as libc::pid_t; CAP];
        let bytes = std::mem::size_of_val(pids.as_slice()) as libc::c_int;
        // SAFETY: the buffer holds `bytes` bytes of zeroed pids.
        let n = unsafe { libc::proc_listpgrppids(pgid, pids.as_mut_ptr().cast(), bytes) };
        if n <= 0 {
            return Vec::new();
        }
        // `n` counts bytes; the buffer was zeroed, so a slot it did not
        // fill is 0 and is dropped.
        pids.truncate((n as usize).min(CAP));
        pids.into_iter()
            .filter_map(|p| u32::try_from(p).ok().filter(|p| *p > 0))
            .collect()
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod imp {
    pub(super) fn start_time(pid: i32) -> Option<u64> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        parse_stat(&stat)
    }

    /// Field 22 of `/proc/<pid>/stat`. Field 2, the command, is in
    /// parentheses and may hold spaces, so counting starts after the last
    /// `)`, at field 3.
    pub(super) fn parse_stat(stat: &str) -> Option<u64> {
        stat_field(stat, 22)
    }

    /// Field `n` (from 3) of a `/proc/<pid>/stat` line.
    pub(super) fn stat_field<T: std::str::FromStr>(stat: &str, n: usize) -> Option<T> {
        let (_, rest) = stat.rsplit_once(')')?;
        rest.split_whitespace().nth(n - 3)?.parse().ok()
    }

    /// Every pid in `/proc` whose field 5, the process group, is `pgid`.
    pub(super) fn group_members(pgid: i32) -> Vec<u32> {
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        dir.flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .filter(|pid| {
                std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .ok()
                    .and_then(|stat| stat_field::<i32>(&stat, 5))
                    == Some(pgid)
            })
            .collect()
    }

    /// Open a pidfd for `pid`, run `same`, and send `signal` through the
    /// pidfd when it holds. `None` when this kernel has no pidfds.
    pub(super) fn pidfd_signal(pid: i32, signal: i32, same: impl Fn() -> bool) -> Option<bool> {
        // SAFETY: plain syscalls; the fd is closed below.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            let err = std::io::Error::last_os_error().raw_os_error();
            // ESRCH: no process has the pid, so there is nothing to signal.
            return (err != Some(libc::ENOSYS)).then_some(false);
        }
        let fd = fd as libc::c_int;
        let sent = same()
            && unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    fd,
                    signal,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            } == 0;
        unsafe { libc::close(fd) };
        Some(sent)
    }

    /// `t` moved onto `CLOCK_BOOTTIME`, which the start time counts in,
    /// through the distance between `t` and now.
    pub(super) fn started_by(pid: i32, t: std::time::SystemTime) -> Option<bool> {
        let ticks = start_time(pid)?;
        // SAFETY: sysconf reads a constant.
        let hz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
        let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
        // SAFETY: `ts` is a valid timespec to fill.
        if hz == 0 || unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) } != 0 {
            return None;
        }
        let boot_now = std::time::Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32);
        let now = std::time::SystemTime::now();
        let t_boot = match now.duration_since(t) {
            Ok(ago) => boot_now.checked_sub(ago)?,
            Err(ahead) => boot_now + ahead.duration(),
        };
        let start = std::time::Duration::from_nanos(ticks.checked_mul(1_000_000_000)? / hz);
        Some(start <= t_boot)
    }

    /// Boot time (`btime` in `/proc/stat`, in seconds) plus the start time
    /// in clock ticks.
    pub(super) fn started_at(pid: i32) -> Option<std::time::SystemTime> {
        let ticks = start_time(pid)?;
        let stat = std::fs::read_to_string("/proc/stat").ok()?;
        let boot: u64 = stat
            .lines()
            .find_map(|l| l.strip_prefix("btime "))?
            .trim()
            .parse()
            .ok()?;
        // SAFETY: sysconf reads a constant.
        let hz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
        if hz == 0 {
            return None;
        }
        let since_boot = std::time::Duration::from_millis(ticks * 1000 / hz);
        Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(boot) + since_boot)
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, FILETIME, HANDLE, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, TerminateProcess, PROCESS_ACCESS_RIGHTS,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    /// `FILETIME` ticks (100 ns) from 1601-01-01 to 1970-01-01.
    const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    const TICKS_PER_SEC: u64 = 10_000_000;

    /// A process handle, closed on drop.
    struct Process(HANDLE);

    impl Process {
        fn open(pid: i32, access: PROCESS_ACCESS_RIGHTS) -> Option<Self> {
            // SAFETY: plain call; a failure answers a null handle.
            let h = unsafe { OpenProcess(access, 0, pid as u32) };
            (!h.is_null()).then_some(Self(h))
        }

        /// Whether the process has not exited. A handle can outlive its
        /// process (a parent holds one until it drops the child), so an
        /// open handle alone does not prove it runs. A process that exited
        /// with code 259 (`STILL_ACTIVE`) itself reads as running.
        fn running(&self) -> bool {
            let mut code = 0u32;
            // SAFETY: the handle is open and `code` is a valid u32 to fill.
            (unsafe { GetExitCodeProcess(self.0, &mut code) }) != 0 && code == STILL_ACTIVE as u32
        }

        /// The creation time as a u64 of `FILETIME` ticks, while the process
        /// has not exited.
        fn start_time(&self) -> Option<u64> {
            if !self.running() {
                return None;
            }
            let zero = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
            // SAFETY: the handle is open and every pointer is a FILETIME.
            let ok = unsafe {
                GetProcessTimes(self.0, &mut created, &mut exited, &mut kernel, &mut user)
            };
            (ok != 0).then(|| {
                (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)
            })
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: the handle is open and closed only here.
            unsafe { CloseHandle(self.0) };
        }
    }

    pub(super) fn start_time(pid: i32) -> Option<u64> {
        Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?.start_time()
    }

    /// The start time is 100 ns ticks since 1601-01-01.
    pub(super) fn started_at(pid: i32) -> Option<std::time::SystemTime> {
        Some(filetime_to_system(start_time(pid)?))
    }

    pub(super) fn filetime_to_system(ticks: u64) -> std::time::SystemTime {
        use std::time::{Duration, UNIX_EPOCH};
        let span = |t: u64| Duration::new(t / TICKS_PER_SEC, ((t % TICKS_PER_SEC) * 100) as u32);
        match ticks.checked_sub(UNIX_EPOCH_TICKS) {
            Some(after) => UNIX_EPOCH + span(after),
            None => UNIX_EPOCH - span(UNIX_EPOCH_TICKS - ticks),
        }
    }

    pub(super) fn started_by(pid: i32, t: std::time::SystemTime) -> Option<bool> {
        Some(started_at(pid)? <= t)
    }

    pub(super) fn group_members(_pgid: i32) -> Vec<u32> {
        Vec::new()
    }

    /// Whether `pid` names a process that has not exited. One this user
    /// may not open exists all the same, as `EPERM` does on Unix.
    pub(super) fn running(pid: i32) -> bool {
        match Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION) {
            Some(p) => p.running(),
            // SAFETY: plain call reading this thread's last error.
            None => (unsafe { GetLastError() }) == ERROR_ACCESS_DENIED,
        }
    }

    /// Open one handle that may both read the creation time and terminate,
    /// and terminate through it only when the creation time is `started`.
    pub(super) fn terminate_same(pid: i32, started: u64) -> bool {
        let access = PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION;
        let Some(p) = Process::open(pid, access) else {
            return false;
        };
        // SAFETY: the handle is open with PROCESS_TERMINATE.
        p.start_time() == Some(started) && unsafe { TerminateProcess(p.0, 1) } != 0
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "android",
    windows
)))]
mod imp {
    pub(super) fn start_time(_pid: i32) -> Option<u64> {
        None
    }

    pub(super) fn started_at(_pid: i32) -> Option<std::time::SystemTime> {
        None
    }

    pub(super) fn started_by(_pid: i32, _t: std::time::SystemTime) -> Option<bool> {
        None
    }

    pub(super) fn group_members(_pgid: i32) -> Vec<u32> {
        Vec::new()
    }
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
        // Linux counts from a boot time in whole seconds.
        assert!(
            theirs + Duration::from_secs(1) >= before,
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
        assert_eq!(imp::filetime_to_system(116_444_736_000_000_000), UNIX_EPOCH);
        assert_eq!(
            imp::filetime_to_system(116_444_736_000_000_000 + 10_000_015),
            UNIX_EPOCH + Duration::new(1, 1_500)
        );
        assert_eq!(
            imp::filetime_to_system(116_444_736_000_000_000 - 10_000_000),
            UNIX_EPOCH - Duration::from_secs(1)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stat_field_22_after_a_command_with_spaces() {
        let stat = "42 (a b) c) S 1 42 42 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 98765 0 0";
        assert_eq!(imp::parse_stat(stat), Some(98765));
        assert_eq!(imp::stat_field::<i32>(stat, 5), Some(42), "the group");
    }
}
