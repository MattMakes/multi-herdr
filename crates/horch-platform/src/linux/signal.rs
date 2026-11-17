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
/// `Some(false)` when no process has the pid. `None` when the pidfd does not
/// open for another reason, so the caller falls back.
fn pidfd_signal(pid: i32, signal: i32, same: impl Fn() -> bool) -> Option<bool> {
    // SAFETY: plain syscalls; the fd is closed below.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return after_open_error(std::io::Error::last_os_error().raw_os_error());
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

/// What a failed `pidfd_open` gives. Only `ESRCH` means that no process
/// has the pid. Each other error (`ENOSYS` before kernel 5.3, `EPERM`,
/// `EMFILE`, `ENFILE`) says nothing about the process, so the caller falls
/// back and never reads a live process as dead.
fn after_open_error(errno: Option<i32>) -> Option<bool> {
    (errno == Some(libc::ESRCH)).then_some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_esrch_from_pidfd_open_means_no_such_process() {
        assert_eq!(after_open_error(Some(libc::ESRCH)), Some(false));
        for errno in [
            libc::ENOSYS,
            libc::EPERM,
            libc::EMFILE,
            libc::ENFILE,
            libc::EINVAL,
        ] {
            assert_eq!(after_open_error(Some(errno)), None, "errno {errno}");
        }
        assert_eq!(after_open_error(None), None);
    }
}
