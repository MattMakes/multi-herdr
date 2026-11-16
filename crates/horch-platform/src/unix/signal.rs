//! The concern signal for the code of macOS and Linux.

use crate::os::signal::signal_same;
use crate::signal::{Stop, Termination};
use crate::NotSupported;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};
use std::sync::Once;

/// Send `signal` to `os_pid`, the OS form of `pid`, only when the start time
/// still matches: the check, then `kill`.
pub(crate) fn check_and_kill(pid: u32, os_pid: i32, started: u64, signal: i32) -> bool {
    // SAFETY: plain syscall on a pid checked to be positive.
    crate::process::start_time(pid) == Some(started) && unsafe { libc::kill(os_pid, signal) } == 0
}

pub(crate) fn stop_same(pid: u32, os_pid: i32, started: u64, how: Stop) -> bool {
    let signal = match how {
        Stop::Term => libc::SIGTERM,
        Stop::Kill => libc::SIGKILL,
    };
    signal_same(pid, os_pid, started, signal)
}

pub(crate) fn terminate_politely(pid: u32, started: u64) -> Result<bool, NotSupported> {
    Ok(crate::signal::signal_same(pid, started, libc::SIGTERM))
}

fn number(t: Termination) -> libc::c_int {
    match t {
        Termination::Terminate => libc::SIGTERM,
        Termination::Interrupt => libc::SIGINT,
        Termination::Hangup => libc::SIGHUP,
    }
}

/// The flag of [`on_termination_set`]. Null until the first call.
static FLAG: AtomicPtr<AtomicBool> = AtomicPtr::new(std::ptr::null_mut());

extern "C" fn set_flag(_: libc::c_int) {
    let flag = FLAG.load(Ordering::SeqCst);
    if !flag.is_null() {
        // SAFETY: the pointer comes from a `&'static AtomicBool`, so it is
        // valid for the whole process. An atomic store is async-signal-safe.
        unsafe { (*flag).store(true, Ordering::SeqCst) };
    }
}

pub(crate) fn on_termination_set(
    flag: &'static AtomicBool,
    on: &[Termination],
) -> Result<(), NotSupported> {
    FLAG.store(std::ptr::from_ref(flag).cast_mut(), Ordering::SeqCst);
    let handler = set_flag as extern "C" fn(libc::c_int) as libc::sighandler_t;
    for t in on {
        // SAFETY: the handler only stores to an atomic, which is
        // async-signal-safe; the signal number is valid.
        unsafe { libc::signal(number(*t), handler) };
    }
    Ok(())
}

/// The group of [`kill_group_on_termination`]. Null until the first call.
static GROUP: AtomicPtr<AtomicI32> = AtomicPtr::new(std::ptr::null_mut());
static INSTALL: Once = Once::new();

extern "C" fn kill_group(sig: libc::c_int) {
    let group = GROUP.load(Ordering::SeqCst);
    let pgid = if group.is_null() {
        0
    } else {
        // SAFETY: the pointer comes from a `&'static AtomicI32`, so it is
        // valid for the whole process. An atomic swap is async-signal-safe.
        unsafe { (*group).swap(0, Ordering::SeqCst) }
    };
    // SAFETY: killpg, signal and raise are async-signal-safe. The default
    // action is put back and the signal raised again, so the process ends as
    // it would have without this handler.
    unsafe {
        if pgid > 0 {
            libc::killpg(pgid, libc::SIGKILL);
        }
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

pub(crate) fn kill_group_on_termination(group: &'static AtomicI32) -> Result<(), NotSupported> {
    GROUP.store(std::ptr::from_ref(group).cast_mut(), Ordering::SeqCst);
    INSTALL.call_once(|| {
        let handler = kill_group as extern "C" fn(libc::c_int) as libc::sighandler_t;
        // SAFETY: the handler only calls async-signal-safe functions; the
        // signal numbers are valid.
        unsafe {
            libc::signal(libc::SIGINT, handler);
            libc::signal(libc::SIGTERM, handler);
            libc::signal(libc::SIGHUP, handler);
        }
    });
    Ok(())
}

extern "C" fn on_hangup(_: libc::c_int) {}

pub(crate) fn outlive_hangup() -> Result<(), NotSupported> {
    let handler = on_hangup as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: the handler does nothing, so it is async-signal-safe.
    unsafe { libc::signal(libc::SIGHUP, handler) };
    Ok(())
}
