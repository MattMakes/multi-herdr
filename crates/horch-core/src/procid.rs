pub fn start_time(pid: u32) -> Option<u64> {
    let pid = raw(pid)?;
    imp::start_time(pid)
}

pub fn started_at(pid: u32) -> Option<std::time::SystemTime> {
    let pid = raw(pid)?;
    imp::started_at(pid)
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

#[cfg(target_os = "macos")]
pub fn is_same_sandboxed(pid: u32, started: u64) -> bool {
    raw(pid).and_then(imp::sysctl_start_time) == Some(started)
}

#[cfg(not(target_os = "macos"))]
pub fn is_same_sandboxed(_pid: u32, _started: u64) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn parent_sandboxed(pid: u32) -> Option<u32> {
    raw(pid).and_then(imp::sysctl_parent)
}

#[cfg(not(target_os = "macos"))]
pub fn parent_sandboxed(_pid: u32) -> Option<u32> {
    None
}

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

pub fn group_members(pgid: u32) -> Vec<u32> {
    match raw(pgid) {
        Some(group) => imp::group_members(group),
        None => Vec::new(),
    }
}

pub fn started_by(pid: u32, t: std::time::SystemTime) -> bool {
    raw(pid).and_then(|pid| imp::started_by(pid, t)) == Some(true)
}

pub fn os_pid(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|p| *p > 0)
}

fn raw(pid: u32) -> Option<i32> {
    os_pid(pid)
}

#[cfg(unix)]
pub fn signal_same(pid: u32, started: u64, signal: i32) -> bool {
    raw(pid).is_some_and(|os| signal_os(pid, os, started, signal))
}

#[cfg(unix)]
fn signal_os(pid: u32, os: i32, started: u64, signal: i32) -> bool {
    #[cfg(target_os = "linux")]
    if let Some(sent) = imp::pidfd_signal(os, signal, || is_same(pid, started)) {
        return sent;
    }
    // SAFETY: plain syscall on a pid checked to be positive.
    is_same(pid, started) && unsafe { libc::kill(os, signal) } == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Term,
    Kill,
}

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

#[cfg(windows)]
pub(crate) fn windows_running(pid: i32) -> bool {
    imp::running(pid)
}

#[cfg(target_os = "macos")]
mod imp {
    pub(super) fn start_time(pid: i32) -> Option<u64> {
        // SAFETY: `proc_bsdinfo` is plain data, and an all-zero value is valid.
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

    const KINFO_PROC_SIZE: usize = 648;
    const KINFO_PROC_PPID: usize = 560;

    fn kinfo_proc(pid: i32) -> Option<[u8; KINFO_PROC_SIZE]> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
        let mut bytes = [0_u8; KINFO_PROC_SIZE];
        let mut len = KINFO_PROC_SIZE;
        // SAFETY: `bytes` has `len` writable bytes, `mib` holds 4 valid
        // integers, and the remaining input pointers are null.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as libc::c_uint,
                bytes.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        (rc == 0 && len == KINFO_PROC_SIZE).then_some(bytes)
    }

    pub(super) fn sysctl_start_time(pid: i32) -> Option<u64> {
        let bytes = kinfo_proc(pid)?;
        // SAFETY: `bytes` holds a whole `kinfo_proc`, which starts with a
        // `timeval`; the buffer need not be aligned, so read it unaligned.
        let started = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<libc::timeval>()) };
        let seconds = u64::try_from(started.tv_sec).ok()?;
        let micros = u64::try_from(started.tv_usec).ok()?;
        (micros < 1_000_000)
            .then_some(())
            .and_then(|()| seconds.checked_mul(1_000_000)?.checked_add(micros))
    }

    pub(super) fn sysctl_parent(pid: i32) -> Option<u32> {
        let bytes = kinfo_proc(pid)?;
        let field = bytes.get(KINFO_PROC_PPID..KINFO_PROC_PPID + 4)?;
        let ppid = i32::from_ne_bytes(field.try_into().ok()?);
        u32::try_from(ppid).ok()
    }

    pub(super) fn started_at(pid: i32) -> Option<std::time::SystemTime> {
        let micros = start_time(pid)?;
        Some(std::time::UNIX_EPOCH + std::time::Duration::from_micros(micros))
    }

    pub(super) fn started_by(pid: i32, t: std::time::SystemTime) -> Option<bool> {
        Some(started_at(pid)? <= t)
    }

    pub(super) fn group_members(pgid: i32) -> Vec<u32> {
        const CAP: usize = 4096;
        let mut pids = vec![0 as libc::pid_t; CAP];
        let bytes = std::mem::size_of_val(pids.as_slice()) as libc::c_int;
        // SAFETY: the buffer holds `bytes` bytes of zeroed pids.
        let n = unsafe { libc::proc_listpgrppids(pgid, pids.as_mut_ptr().cast(), bytes) };
        if n <= 0 {
            return Vec::new();
        }
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

    pub(super) fn parse_stat(stat: &str) -> Option<u64> {
        stat_field(stat, 22)
    }

    pub(super) fn stat_field<T: std::str::FromStr>(stat: &str, n: usize) -> Option<T> {
        let (_, rest) = stat.rsplit_once(')')?;
        rest.split_whitespace().nth(n - 3)?.parse().ok()
    }

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

    pub(super) fn pidfd_signal(pid: i32, signal: i32, same: impl Fn() -> bool) -> Option<bool> {
        // SAFETY: plain syscalls; the fd is closed below.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            let err = std::io::Error::last_os_error().raw_os_error();
            return (err != Some(libc::ENOSYS)).then_some(false);
        }
        let fd = fd as libc::c_int;
        // SAFETY: `fd` is the pidfd opened above and still open; the siginfo
        // pointer is null, which the syscall allows.
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
        // SAFETY: `fd` is the pidfd this function opened and closes only here.
        unsafe { libc::close(fd) };
        Some(sent)
    }

    pub(super) fn started_by(pid: i32, t: std::time::SystemTime) -> Option<bool> {
        let ticks = start_time(pid)?;
        // SAFETY: sysconf reads a constant.
        let hz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
        // SAFETY: timespec is plain integers; all-zero bytes are a valid value.
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

    const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    const TICKS_PER_SEC: u64 = 10_000_000;

    struct Process(HANDLE);

    impl Process {
        fn open(pid: i32, access: PROCESS_ACCESS_RIGHTS) -> Option<Self> {
            // SAFETY: plain call; a failure answers a null handle.
            let h = unsafe { OpenProcess(access, 0, pid as u32) };
            (!h.is_null()).then_some(Self(h))
        }

        fn running(&self) -> bool {
            let mut code = 0u32;
            // SAFETY: the handle is open and `code` is a valid u32 to fill.
            (unsafe { GetExitCodeProcess(self.0, &mut code) }) != 0 && code == STILL_ACTIVE as u32
        }

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

    pub(super) fn running(pid: i32) -> bool {
        match Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION) {
            Some(p) => p.running(),
            // SAFETY: plain call reading this thread's last error.
            None => (unsafe { GetLastError() }) == ERROR_ACCESS_DENIED,
        }
    }

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

    #[cfg(target_os = "macos")]
    #[test]
    fn the_sandbox_safe_sysctl_names_the_same_process_start() {
        let me = std::process::id();
        assert_eq!(
            imp::sysctl_start_time(os_pid(me).unwrap()),
            start_time(me),
            "the sandbox fallback keeps the recorded identity"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_sandbox_safe_sysctl_names_the_parent() {
        let me = os_pid(std::process::id()).unwrap();
        // SAFETY: `getppid` has no preconditions.
        let parent = unsafe { libc::getppid() };
        assert_eq!(imp::sysctl_parent(me), u32::try_from(parent).ok());
        assert_eq!(imp::sysctl_parent(i32::MAX), None, "no such process");
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
