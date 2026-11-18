//! The concern process for macOS.

use crate::NotSupported;
use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) use crate::unix::process::alive;

/// The BSD info of `pid` from `proc_pidinfo`. `None` when no process has
/// the pid.
fn bsd_info(pid: i32) -> Option<libc::proc_bsdinfo> {
    // SAFETY: `proc_bsdinfo` is plain data, and an all-zero value is valid.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is a zeroed `proc_bsdinfo` of `size` bytes.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (n == size).then_some(info)
}

/// Microseconds since the epoch.
pub(crate) fn start_time(pid: i32) -> Option<u64> {
    bsd_info(pid).map(|info| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

pub(crate) fn started_at(pid: i32) -> Option<SystemTime> {
    let micros = start_time(pid)?;
    Some(UNIX_EPOCH + Duration::from_micros(micros))
}

pub(crate) fn started_by(pid: i32, t: SystemTime) -> Option<bool> {
    Some(started_at(pid)? <= t)
}

pub(crate) fn parent(pid: u32) -> Result<Option<u32>, NotSupported> {
    let Some(os) = i32::try_from(pid).ok().filter(|p| *p > 0) else {
        return Ok(None);
    };
    Ok(bsd_info(os).map(|info| info.pbi_ppid))
}

pub(crate) fn group_members(pgid: i32) -> Result<Vec<u32>, NotSupported> {
    // Room for more members than a judge job has; a fuller group is cut.
    const CAP: usize = 4096;
    let mut pids = vec![0 as libc::pid_t; CAP];
    let bytes = std::mem::size_of_val(pids.as_slice()) as libc::c_int;
    // SAFETY: the buffer holds `bytes` bytes of zeroed pids.
    let n = unsafe { libc::proc_listpgrppids(pgid, pids.as_mut_ptr().cast(), bytes) };
    if n <= 0 {
        return Ok(Vec::new());
    }
    // `n` counts bytes; the buffer was zeroed, so a slot it did not fill is
    // 0 and is dropped.
    pids.truncate((n as usize).min(CAP));
    Ok(pids
        .into_iter()
        .filter_map(|p| u32::try_from(p).ok().filter(|p| *p > 0))
        .collect())
}

pub(crate) fn cwd_pids_under(dir: &Path) -> Result<io::Result<Vec<u32>>, NotSupported> {
    Ok(all_pids().map(|pids| {
        pids.into_iter()
            .filter(|pid| cwd(*pid).is_some_and(|c| c.starts_with(dir)))
            .filter_map(|pid| u32::try_from(pid).ok())
            .collect()
    }))
}

/// Each pid of the machine, from `proc_listallpids`. The buffer grows
/// until the list fits, so a list is never cut.
fn all_pids() -> io::Result<Vec<libc::pid_t>> {
    // SAFETY: a null buffer of 0 bytes asks for the count only.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(io::Error::last_os_error());
    }
    let mut cap = count as usize + 64;
    loop {
        let mut pids = vec![0 as libc::pid_t; cap];
        let bytes = libc::c_int::try_from(std::mem::size_of_val(pids.as_slice()))
            .map_err(|_| io::Error::other("too many processes"))?;
        // SAFETY: the buffer holds `bytes` bytes of zeroed pids.
        let n = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        if n <= 0 {
            return Err(io::Error::last_os_error());
        }
        // `n` counts pids. A full buffer can be a cut list: try a larger one.
        if (n as usize) < cap {
            pids.truncate(n as usize);
            pids.retain(|p| *p > 0);
            return Ok(pids);
        }
        cap *= 2;
    }
}

/// The working folder of `pid`, or `None` when this user cannot read it.
fn cwd(pid: libc::pid_t) -> Option<PathBuf> {
    // SAFETY: `proc_vnodepathinfo` is plain data, and an all-zero value is
    // valid.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: the buffer is a zeroed `proc_vnodepathinfo` of `size` bytes.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut libc::proc_vnodepathinfo).cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    let path = &info.pvi_cdir.vip_path;
    // SAFETY: `vip_path` is `MAXPATHLEN` contiguous bytes inside `info`.
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(path.as_ptr().cast(), std::mem::size_of_val(path)) };
    let end = bytes.iter().position(|b| *b == 0)?;
    (end > 0).then(|| PathBuf::from(OsStr::from_bytes(&bytes[..end])))
}
