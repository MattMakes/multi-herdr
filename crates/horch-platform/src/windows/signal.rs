//! The concern signal for Windows. Windows has `TerminateProcess` only: no
//! polite stop and no signal handlers. Those functions give
//! [`NotSupported`].

use crate::os::process::Process;
use crate::signal::Stop;
use crate::NotSupported;
use std::sync::atomic::{AtomicBool, AtomicI32};
use windows_sys::Win32::System::Threading::{
    TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};

const NOT_SUPPORTED: NotSupported = NotSupported::new("signal", "windows");

/// Open one handle that may both read the creation time and terminate, and
/// terminate through it with exit code 1 only when the creation time is
/// `started`. `how` makes no difference.
pub(crate) fn stop_same(_pid: u32, os_pid: i32, started: u64, _how: Stop) -> bool {
    let access = PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION;
    let Some(p) = Process::open(os_pid, access) else {
        return false;
    };
    // SAFETY: the handle is open with PROCESS_TERMINATE while `p` lives.
    p.start_time() == Some(started) && unsafe { TerminateProcess(p.handle(), 1) } != 0
}

pub(crate) fn signal_if_same(_pid: u32, _started: u64, _signal: i32) -> Result<bool, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn terminate_politely(_pid: u32, _started: u64) -> Result<bool, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn on_termination_set(
    _flag: &'static AtomicBool,
    _on: &[crate::signal::Termination],
) -> Result<(), NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn kill_group_on_termination(_group: &'static AtomicI32) -> Result<(), NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn hold_termination() {}

pub(crate) fn end_termination_hold() {}

pub(crate) fn outlive_hangup() -> Result<(), NotSupported> {
    Err(NOT_SUPPORTED)
}
