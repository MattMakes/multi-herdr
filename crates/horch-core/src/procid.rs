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
//! Linux gives clock ticks since boot (`/proc/<pid>/stat` field 22). Other
//! platforms, Windows among them, have no start time here: [`start_time`] is
//! `None` and [`alive`] falls back to "the pid exists".

/// The start time of `pid`, or `None` when it does not run or this platform
/// cannot read it.
pub fn start_time(pid: u32) -> Option<u64> {
    let pid = raw(pid)?;
    imp::start_time(pid)
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

/// `pid` as the OS type, when it can name one process: 0 and values that
/// would turn negative (a process group, or every process) are refused.
fn raw(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|p| *p > 0)
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
        let (_, rest) = stat.rsplit_once(')')?;
        rest.split_whitespace().nth(22 - 3)?.parse().ok()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
mod imp {
    pub(super) fn start_time(_pid: i32) -> Option<u64> {
        None
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

    #[cfg(target_os = "linux")]
    #[test]
    fn stat_field_22_after_a_command_with_spaces() {
        let stat = "42 (a b) c) S 1 42 42 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 98765 0 0";
        assert_eq!(imp::parse_stat(stat), Some(98765));
    }
}
