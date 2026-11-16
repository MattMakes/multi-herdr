//! The concern signal for Linux. A signal goes through a pidfd when the
//! kernel has them (5.3 and later).

pub(crate) use crate::unix::signal::{
    kill_group_on_termination, on_termination_set, outlive_hangup, signal_if_same, stop_same,
    terminate_politely,
};

/// The pidfd route, else the check and `kill` of the shared unix code.
pub(crate) fn signal_same(pid: u32, os_pid: i32, started: u64, signal: i32) -> bool {
    if let Some(sent) = pidfd_signal(os_pid, signal, || {
        crate::process::start_time(pid) == Some(started)
    }) {
        return sent;
    }
    crate::unix::signal::check_and_kill(pid, os_pid, started, signal)
}

/// Open a pidfd for `pid`, then send `signal` through it when `same` holds.
/// `None` when the kernel has no pidfds (`ENOSYS`), so the caller falls
/// back. `Some(false)` when no process has the pid.
fn pidfd_signal(pid: i32, signal: i32, same: impl Fn() -> bool) -> Option<bool> {
    // SAFETY: plain syscalls; the fd is closed below.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        let err = std::io::Error::last_os_error().raw_os_error();
        // ESRCH: no process has the pid, so there is nothing to signal.
        return (err != Some(libc::ENOSYS)).then_some(false);
    }
    let fd = fd as libc::c_int;
    // SAFETY: `fd` is the pidfd opened above and still open; the siginfo
    // pointer is null, which the syscall allows.
    let sent = same()
        && unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd,
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        } == 0;
    // SAFETY: `fd` is the pidfd this function opened and closes only here.
    unsafe { libc::close(fd) };
    Some(sent)
}
