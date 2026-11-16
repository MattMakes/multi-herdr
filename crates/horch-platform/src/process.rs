//! The concern process: process identity and liveness: alive, start time,
//! parent and the members of a process group.
//!
//! A start time is an opaque `u64` in the unit of the OS: microseconds since
//! the epoch on macOS, clock ticks since boot on Linux and `FILETIME` ticks
//! on Windows. Compare 2 start times of the same machine for equality only.

use crate::os::process as os;
use crate::NotSupported;
use std::time::SystemTime;

/// The pid as the OS takes it. 0 and values above `i32::MAX` name a group
/// or every process, not one process.
fn raw(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|p| *p > 0)
}

/// The start time of process `pid`. `None` when no process has the pid.
pub fn start_time(pid: u32) -> Option<u64> {
    os::start_time(raw(pid)?)
}

/// The start of process `pid` as a wall-clock time.
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
