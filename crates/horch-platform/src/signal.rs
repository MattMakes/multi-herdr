//! The concern signal: signals, the stop of a process, and the handlers of
//! the termination signals.
//!
//! A signal or a stop goes to a pid only while the process has its
//! recorded start time ([`crate::process::start_time`]). A pid alone can
//! name a later program, and a wrong stop kills a process that is not ours.
//!
//! Each handler that this concern installs only touches atomics or calls
//! async-signal-safe functions.

use crate::os::signal as os;
use crate::NotSupported;
use std::sync::atomic::{AtomicBool, AtomicI32};

/// The pid as the OS takes it. 0 and values above `i32::MAX` name a group
/// or every process, not one process.
fn raw(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|p| *p > 0)
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

/// A signal that asks a process to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    /// `SIGTERM`.
    Terminate,
    /// `SIGINT`.
    Interrupt,
    /// `SIGHUP`.
    Hangup,
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
///
/// Windows gives [`NotSupported`]: it has no signals.
/// `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
/// row "Signals and stop" ("no signal handlers"), names the gap.
pub fn signal_same(pid: u32, started: u64, signal: i32) -> Result<bool, NotSupported> {
    os::signal_if_same(pid, started, signal)
}

/// Stop `pid` only while it runs with start time `started`, and say whether
/// a stop was sent.
///
/// On Unix this is [`signal_same`] with `SIGTERM` or `SIGKILL`. On Windows
/// one handle is opened with `PROCESS_TERMINATE` and
/// `PROCESS_QUERY_LIMITED_INFORMATION`, the creation time is read through
/// that handle, and only on a match is `TerminateProcess` called on the same
/// handle. The handle names the process that had the pid when it was opened
/// and keeps that pid from being given out, so a reused pid can never be hit
/// between the check and the stop. Both kinds of stop end the process at
/// once there, with exit code 1.
pub fn stop_same(pid: u32, started: u64, how: Stop) -> bool {
    raw(pid).is_some_and(|os_pid| os::stop_same(pid, os_pid, started, how))
}

/// Ask `pid` to end, only while it runs with start time `started`, and say
/// whether the request was sent. On Unix this is `SIGTERM` through
/// [`signal_same`]: the process can catch it and clean up. This function
/// does not wait for the process to end.
///
/// Windows gives [`NotSupported`]: it has no polite stop for an arbitrary
/// process. `docs/specs/fleetd/preservation-06-wiring-and-platform.md`,
/// section 6, row "Signals and stop" ("Windows: no graceful stop, no stop
/// of stale watchdog or collector"), names the gap.
pub fn terminate_politely(pid: u32, started: u64) -> Result<bool, NotSupported> {
    os::terminate_politely(pid, started)
}

/// Set `flag` when one of the signals `on` comes. The handler only stores
/// `true` to the flag, so the caller ends its loop at a point it chooses.
/// The process has 1 such flag: a later call points every handler that
/// this function installs at the flag of that call.
///
/// Windows gives [`NotSupported`]: it has no signal handlers.
/// `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
/// row "Signals and stop" ("no signal handlers"), names the gap.
pub fn on_termination_set(
    flag: &'static AtomicBool,
    on: &[Termination],
) -> Result<(), NotSupported> {
    os::on_termination_set(flag, on)
}

/// On `SIGINT`, `SIGTERM` or `SIGHUP`, kill the process group in `group`
/// with `SIGKILL` when it is above 0, then end this process by the same
/// signal with its default action, as without the handler. The handler sets
/// `group` to 0 when it takes it. The caller stores the group of the work
/// that must not outlive it, and 0 when none runs. The handlers are
/// installed at the first call only. A later call points them at the
/// `group` of that call.
///
/// Windows gives [`NotSupported`]: it has no signal handlers and no
/// process groups. `docs/specs/fleetd/preservation-06-wiring-and-platform.md`,
/// section 6, row "Signals and stop" ("no signal handlers"), names the gap.
pub fn kill_group_on_termination(group: &'static AtomicI32) -> Result<(), NotSupported> {
    os::kill_group_on_termination(group)
}

/// Let this process live on after `SIGHUP`, the signal of a closed
/// terminal. A handler that does nothing, not `SIG_IGN`: an exec puts the
/// default action back, so a program that this process starts still ends
/// on the hangup.
///
/// Windows gives [`NotSupported`]: it has no signal handlers.
/// `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
/// row "Signals and stop" ("no signal handlers"), names the gap.
pub fn outlive_hangup() -> Result<(), NotSupported> {
    os::outlive_hangup()
}
