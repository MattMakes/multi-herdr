//! The concern process for macOS.

use crate::NotSupported;
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
