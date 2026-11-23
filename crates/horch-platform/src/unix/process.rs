pub(crate) fn alive(pid: i32) -> bool {
    // SAFETY: `kill` with signal 0 sends nothing and touches no memory; `pid` is a pid value.
    let r = unsafe { libc::kill(pid, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub(crate) fn group_of(pid: i32) -> Result<Option<u32>, crate::NotSupported> {
    // SAFETY: `getpgid` reads 1 value of the process table and touches no memory.
    let g = unsafe { libc::getpgid(pid) };
    Ok(u32::try_from(g).ok())
}

pub(crate) fn session_of(pid: i32) -> Result<Option<u32>, crate::NotSupported> {
    // SAFETY: `getsid` reads 1 value of the process table and touches no memory.
    let s = unsafe { libc::getsid(pid) };
    Ok(u32::try_from(s).ok())
}
