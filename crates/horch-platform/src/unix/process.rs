//! The concern process for the code of macOS and Linux.

/// Whether `pid` exists. Signal 0 checks existence and permission without
/// sending anything. `EPERM` means the process exists but this user may
/// not signal it.
pub(crate) fn alive(pid: i32) -> bool {
    // SAFETY: `kill` with signal 0 sends nothing and touches no memory; `pid` is a pid value.
    let r = unsafe { libc::kill(pid, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}
