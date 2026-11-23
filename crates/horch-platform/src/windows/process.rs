use crate::NotSupported;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ACCESS_DENIED, FILETIME, HANDLE, STILL_ACTIVE,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_ACCESS_RIGHTS,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
const TICKS_PER_SEC: u64 = 10_000_000;

const NOT_SUPPORTED: NotSupported = NotSupported::new("process", "windows");

pub(crate) struct Process(HANDLE);

impl Process {
    pub(crate) fn handle(&self) -> HANDLE {
        self.0
    }

    pub(crate) fn open(pid: i32, access: PROCESS_ACCESS_RIGHTS) -> Option<Self> {
        // SAFETY: plain call; a failure answers a null handle.
        let h = unsafe { OpenProcess(access, 0, pid as u32) };
        (!h.is_null()).then_some(Self(h))
    }

    fn running(&self) -> bool {
        let mut code = 0u32;
        // SAFETY: the handle is open and `code` is a valid u32 to fill.
        (unsafe { GetExitCodeProcess(self.0, &mut code) }) != 0 && code == STILL_ACTIVE as u32
    }

    pub(crate) fn start_time(&self) -> Option<u64> {
        if !self.running() {
            return None;
        }
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: the handle is open and every pointer is a FILETIME.
        let ok =
            unsafe { GetProcessTimes(self.0, &mut created, &mut exited, &mut kernel, &mut user) };
        (ok != 0)
            .then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle is open and closed only here.
        unsafe { CloseHandle(self.0) };
    }
}

pub(crate) fn start_time(pid: i32) -> Option<u64> {
    Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?.start_time()
}

pub(crate) fn started_at(pid: i32) -> Option<SystemTime> {
    Some(filetime_to_system(start_time(pid)?))
}

pub(crate) fn filetime_to_system(ticks: u64) -> SystemTime {
    let span = |t: u64| Duration::new(t / TICKS_PER_SEC, ((t % TICKS_PER_SEC) * 100) as u32);
    match ticks.checked_sub(UNIX_EPOCH_TICKS) {
        Some(after) => UNIX_EPOCH + span(after),
        None => UNIX_EPOCH - span(UNIX_EPOCH_TICKS - ticks),
    }
}

pub(crate) fn started_by(pid: i32, t: SystemTime) -> Option<bool> {
    Some(started_at(pid)? <= t)
}

pub(crate) fn alive(pid: i32) -> bool {
    match Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION) {
        Some(p) => p.running(),
        // SAFETY: plain call reading this thread's last error.
        None => (unsafe { GetLastError() }) == ERROR_ACCESS_DENIED,
    }
}

pub(crate) fn parent(_pid: u32) -> Result<Option<u32>, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn group_of(_pid: i32) -> Result<Option<u32>, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn session_of(_pid: i32) -> Result<Option<u32>, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn group_members(_pgid: i32) -> Result<Vec<u32>, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn cwd_pids_under(
    _dir: &std::path::Path,
) -> Result<std::io::Result<Vec<u32>>, NotSupported> {
    Err(NOT_SUPPORTED)
}
