//! The concern process: process identity and liveness: alive, start time,
//! parent and the members of a process group.
//!
//! A start time is an opaque `u64` in the unit of the OS: microseconds since
//! the epoch on macOS, clock ticks since boot on Linux and `FILETIME` ticks
//! on Windows. Compare 2 start times of the same machine for equality only.

use crate::os::process as os;
use crate::NotSupported;
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// The pid as the OS takes it. 0 and values above `i32::MAX` name a group
/// or every process, not one process.
fn raw(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|p| *p > 0)
}

/// The start time of process `pid`. `None` when no process has the pid,
/// and also when the process exists but the OS query fails. `None` does
/// not prove that the process is dead: use [`alive`] for that.
pub fn start_time(pid: u32) -> Option<u64> {
    os::start_time(raw(pid)?)
}

/// The start of process `pid` as a wall-clock time. `None` in the same
/// cases as [`start_time`].
pub fn started_at(pid: u32) -> Option<SystemTime> {
    os::started_at(raw(pid)?)
}

/// Whether process `pid` started at or before `t`. False when the start
/// time cannot be read.
pub fn started_by(pid: u32, t: SystemTime) -> bool {
    raw(pid).and_then(|pid| os::started_by(pid, t)) == Some(true)
}

/// Whether process `pid` exists. A process that this user may not signal
/// or open exists all the same. A pid alone can name a later program:
/// compare [`start_time`] too when that matters.
pub fn alive(pid: u32) -> bool {
    raw(pid).is_some_and(os::alive)
}

/// The parent pid of process `pid`. `Ok(None)` when it cannot be read.
///
/// Windows gives [`NotSupported`]: `docs/specs/fleetd/preservation-06-wiring-and-platform.md`,
/// section 6, row "Process identity and liveness" ("Windows: no parent pid").
pub fn parent(pid: u32) -> Result<Option<u32>, NotSupported> {
    os::parent(pid)
}

/// The start time of process `pid` from the macOS `KERN_PROC_PID` sysctl,
/// which Claude's sandbox permits where it blocks `proc_pidinfo`. The same
/// unit as [`start_time`]. `None` on other platforms.
pub fn start_time_sandboxed(pid: u32) -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        os::sysctl_start_time(raw(pid)?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// The parent pid of process `pid` from the macOS `KERN_PROC_PID` sysctl,
/// for a sandbox that blocks [`parent`]. `None` on other platforms.
pub fn parent_sandboxed(pid: u32) -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        os::sysctl_parent(raw(pid)?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// The pids in process group `pgid`. Empty when it cannot be read.
///
/// Windows gives [`NotSupported`]: it has no process groups.
/// `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
/// row "Process identity and liveness", names the gap.
pub fn group_members(pgid: u32) -> Result<Vec<u32>, NotSupported> {
    match raw(pgid) {
        Some(group) => os::group_members(group),
        None => Ok(Vec::new()),
    }
}

/// The process group of process `pid`. `Ok(None)` when it cannot be read.
///
/// Windows gives [`NotSupported`]: it has no process groups.
/// `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
/// row "Process identity and liveness", names the gap.
pub fn group_of(pid: u32) -> Result<Option<u32>, NotSupported> {
    match raw(pid) {
        Some(pid) => os::group_of(pid),
        None => Ok(None),
    }
}

/// The session of process `pid`: the pid of its session leader. `Ok(None)`
/// when it cannot be read.
///
/// Windows gives [`NotSupported`]: it has no sessions of this kind.
pub fn session_of(pid: u32) -> Result<Option<u32>, NotSupported> {
    match raw(pid) {
        Some(pid) => os::session_of(pid),
        None => Ok(None),
    }
}

/// The pids of the processes that this user can see whose working folder
/// is `dir` or a folder in it. Give `dir` as a canonical path: the OS gives
/// canonical working folders. A process whose working folder cannot be read
/// is not in the list. `Ok(Err(_))` when the list of processes cannot be
/// read.
///
/// Windows gives [`NotSupported`]: no public API gives the working folder of
/// another process.
pub fn cwd_pids_under(dir: &Path) -> Result<io::Result<Vec<u32>>, NotSupported> {
    os::cwd_pids_under(dir)
}

/// Field 22, the start time, of a `/proc/<pid>/stat` line.
#[cfg(target_os = "linux")]
pub fn parse_stat(stat: &str) -> Option<u64> {
    os::parse_stat(stat)
}

/// Field `n` (from 3) of a `/proc/<pid>/stat` line.
#[cfg(target_os = "linux")]
pub fn stat_field<T: std::str::FromStr>(stat: &str, n: usize) -> Option<T> {
    os::stat_field(stat, n)
}

/// A time of `FILETIME` ticks (100 ns since 1601-01-01) as a `SystemTime`.
#[cfg(windows)]
pub fn filetime_to_system(ticks: u64) -> SystemTime {
    os::filetime_to_system(ticks)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn cwd_pids_under_finds_a_child_in_the_folder_and_not_outside_it() {
        let dir = std::env::temp_dir().join(format!("cwd-pids-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(dir.join("sub"))
            .spawn()
            .unwrap();
        let mut seen = Vec::new();
        for _ in 0..50 {
            seen = cwd_pids_under(&dir).unwrap().unwrap();
            if seen.contains(&child.id()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let other = cwd_pids_under(&dir.join("other")).unwrap().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(seen.contains(&child.id()), "{seen:?}");
        assert!(!other.contains(&child.id()), "{other:?}");
    }
}
