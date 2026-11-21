use crate::os::signal::signal_same;
use crate::signal::{Stop, Termination};
use crate::NotSupported;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering};
use std::sync::Once;

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

pub(crate) fn signal_if_same(pid: u32, started: u64, signal: i32) -> Result<bool, NotSupported> {
    let os_pid = i32::try_from(pid).ok().filter(|p| *p > 0);
    Ok(os_pid.is_some_and(|os_pid| signal_same(pid, os_pid, started, signal)))
}

pub(crate) fn terminate_politely(pid: u32, started: u64) -> Result<bool, NotSupported> {
    crate::signal::signal_same(pid, started, libc::SIGTERM)
}

fn number(t: Termination) -> libc::c_int {
    match t {
        Termination::Terminate => libc::SIGTERM,
        Termination::Interrupt => libc::SIGINT,
        Termination::Hangup => libc::SIGHUP,
    }
}

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

static GROUPS: [AtomicPtr<AtomicI32>; GROUP_SLOTS] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; GROUP_SLOTS];
const GROUP_SLOTS: usize = 8;
static INSTALL: Once = Once::new();

static HOLD: AtomicU32 = AtomicU32::new(0);
const SIGNAL_BITS: u32 = 8;
const SIGNAL_MASK: u32 = (1 << SIGNAL_BITS) - 1;

extern "C" fn kill_group(sig: libc::c_int) {
    let held = u32::try_from(sig).unwrap_or(0) & SIGNAL_MASK;
    let mut now = HOLD.load(Ordering::SeqCst);
    while now >> SIGNAL_BITS > 0 {
        if now & SIGNAL_MASK != 0 {
            return;
        }
        match HOLD.compare_exchange(now, now | held, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return,
            Err(seen) => now = seen,
        }
    }
    kill_groups_and_end(sig);
}

fn kill_groups_and_end(sig: libc::c_int) {
    for slot in &GROUPS {
        let group = slot.load(Ordering::SeqCst);
        if group.is_null() {
            continue;
        }
        // SAFETY: the pointer comes from a `&'static AtomicI32`, so it is
        // valid for the whole process. An atomic swap and killpg are
        // async-signal-safe.
        unsafe {
            let pgid = (*group).swap(0, Ordering::SeqCst);
            if pgid > 0 {
                libc::killpg(pgid, libc::SIGKILL);
            }
        }
    }
    // SAFETY: signal and raise are async-signal-safe. The default action is
    // put back and the signal raised again, so the process ends as it would
    // have without this handler.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

fn register(group: &'static AtomicI32) {
    let ptr = std::ptr::from_ref(group).cast_mut();
    if GROUPS.iter().any(|s| s.load(Ordering::SeqCst) == ptr) {
        return;
    }
    let taken = GROUPS.iter().any(|s| {
        s.compare_exchange(
            std::ptr::null_mut(),
            ptr,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
    });
    assert!(
        taken,
        "more than {GROUP_SLOTS} groups for kill_group_on_termination"
    );
}

pub(crate) fn kill_group_on_termination(group: &'static AtomicI32) -> Result<(), NotSupported> {
    register(group);
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

pub(crate) fn hold_termination() {
    HOLD.fetch_add(1 << SIGNAL_BITS, Ordering::SeqCst);
}

pub(crate) fn end_termination_hold() {
    let mut now = HOLD.load(Ordering::SeqCst);
    loop {
        let holds = (now >> SIGNAL_BITS).saturating_sub(1);
        let sig = now & SIGNAL_MASK;
        let next = if holds == 0 {
            0
        } else {
            now - (1 << SIGNAL_BITS)
        };
        match HOLD.compare_exchange(now, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => {
                if holds == 0 && sig != 0 {
                    kill_groups_and_end(i32::try_from(sig).unwrap_or(libc::SIGTERM));
                }
                return;
            }
            Err(seen) => now = seen,
        }
    }
}

extern "C" fn on_hangup(_: libc::c_int) {}

pub(crate) fn outlive_hangup() -> Result<(), NotSupported> {
    let handler = on_hangup as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: the handler does nothing, so it is async-signal-safe.
    unsafe { libc::signal(libc::SIGHUP, handler) };
    Ok(())
}
