use crate::os::process as os;
use crate::NotSupported;
use std::io;
use std::path::Path;
use std::time::SystemTime;

fn raw(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|p| *p > 0)
}

pub fn start_time(pid: u32) -> Option<u64> {
    os::start_time(raw(pid)?)
}

pub fn started_at(pid: u32) -> Option<SystemTime> {
    os::started_at(raw(pid)?)
}

pub fn started_by(pid: u32, t: SystemTime) -> bool {
    raw(pid).and_then(|pid| os::started_by(pid, t)) == Some(true)
}

pub fn alive(pid: u32) -> bool {
    raw(pid).is_some_and(os::alive)
}

pub fn parent(pid: u32) -> Result<Option<u32>, NotSupported> {
    os::parent(pid)
}

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

pub fn group_members(pgid: u32) -> Result<Vec<u32>, NotSupported> {
    match raw(pgid) {
        Some(group) => os::group_members(group),
        None => Ok(Vec::new()),
    }
}

pub fn group_of(pid: u32) -> Result<Option<u32>, NotSupported> {
    match raw(pid) {
        Some(pid) => os::group_of(pid),
        None => Ok(None),
    }
}

pub fn session_of(pid: u32) -> Result<Option<u32>, NotSupported> {
    match raw(pid) {
        Some(pid) => os::session_of(pid),
        None => Ok(None),
    }
}

pub fn cwd_pids_under(dir: &Path) -> Result<io::Result<Vec<u32>>, NotSupported> {
    os::cwd_pids_under(dir)
}

#[cfg(target_os = "linux")]
pub fn parse_stat(stat: &str) -> Option<u64> {
    os::parse_stat(stat)
}

#[cfg(target_os = "linux")]
pub fn stat_field<T: std::str::FromStr>(stat: &str, n: usize) -> Option<T> {
    os::stat_field(stat, n)
}

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
