use crate::os::signal as os;
use crate::NotSupported;
use std::sync::atomic::{AtomicBool, AtomicI32};

fn raw(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|p| *p > 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Term,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    Terminate,
    Interrupt,
    Hangup,
}

pub fn signal_same(pid: u32, started: u64, signal: i32) -> Result<bool, NotSupported> {
    os::signal_if_same(pid, started, signal)
}

pub fn stop_same(pid: u32, started: u64, how: Stop) -> bool {
    raw(pid).is_some_and(|os_pid| os::stop_same(pid, os_pid, started, how))
}

pub fn terminate_politely(pid: u32, started: u64) -> Result<bool, NotSupported> {
    os::terminate_politely(pid, started)
}

pub fn on_termination_set(
    flag: &'static AtomicBool,
    on: &[Termination],
) -> Result<(), NotSupported> {
    os::on_termination_set(flag, on)
}

pub fn kill_group_on_termination(group: &'static AtomicI32) -> Result<(), NotSupported> {
    os::kill_group_on_termination(group)
}

pub fn hold_termination() -> TerminationHold {
    os::hold_termination();
    TerminationHold { _open: () }
}

#[must_use = "the hold ends when the guard drops"]
pub struct TerminationHold {
    _open: (),
}

impl Drop for TerminationHold {
    fn drop(&mut self) {
        os::end_termination_hold();
    }
}

pub fn outlive_hangup() -> Result<(), NotSupported> {
    os::outlive_hangup()
}
