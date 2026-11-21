//! The concern process for macOS.

use crate::NotSupported;
use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) use crate::unix::process::{alive, group_of, session_of};

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

/// The size of Darwin's `kinfo_proc` on 64-bit macOS.
const KINFO_PROC_SIZE: usize = 648;
/// The offset of `kp_eproc.e_ppid` in `kinfo_proc`.
const KINFO_PROC_PPID: usize = 560;

/// `kern.proc.pid.<pid>`: the `kinfo_proc` of `pid`. Claude's macOS
/// sandbox permits this query where it blocks `proc_pidinfo`.
fn kinfo_proc(pid: i32) -> Option<[u8; KINFO_PROC_SIZE]> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
    let mut bytes = [0_u8; KINFO_PROC_SIZE];
    let mut len = KINFO_PROC_SIZE;
    // SAFETY: `bytes` has `len` writable bytes, `mib` holds 4 valid
    // integers, and the remaining input pointers are null.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            bytes.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && len == KINFO_PROC_SIZE).then_some(bytes)
}

/// The start time from `kinfo_proc`: `kp_proc.p_starttime`, a `timeval`
/// at offset 0. Microseconds since the epoch, as [`start_time`].
pub(crate) fn sysctl_start_time(pid: i32) -> Option<u64> {
    let bytes = kinfo_proc(pid)?;
    // SAFETY: `bytes` holds a whole `kinfo_proc`, which starts with a
    // `timeval`; the buffer need not be aligned, so read it unaligned.
    let started = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<libc::timeval>()) };
    let seconds = u64::try_from(started.tv_sec).ok()?;
    let micros = u64::try_from(started.tv_usec).ok()?;
    (micros < 1_000_000)
        .then_some(())
        .and_then(|()| seconds.checked_mul(1_000_000)?.checked_add(micros))
}

/// The parent pid from `kinfo_proc`: `kp_eproc.e_ppid`.
pub(crate) fn sysctl_parent(pid: i32) -> Option<u32> {
    let bytes = kinfo_proc(pid)?;
    let field = bytes.get(KINFO_PROC_PPID..KINFO_PROC_PPID + 4)?;
    let ppid = i32::from_ne_bytes(field.try_into().ok()?);
    u32::try_from(ppid).ok()
}

pub(crate) fn started_at(pid: i32) -> Option<SystemTime> {
    let micros = start_time(pid)?;
    Some(UNIX_EPOCH + Duration::from_micros(micros))
}

pub(crate) fn started_by(pid: i32, t: SystemTime) -> Option<bool> {
    Some(started_at(pid)? <= t)
}

/// The short BSD info, not [`bsd_info`]: a user can read the short info of
/// a process of another user, such as `login` of root, so a parent chain
/// can reach PID 1.
pub(crate) fn parent(pid: u32) -> Result<Option<u32>, NotSupported> {
    let Some(os) = i32::try_from(pid).ok().filter(|p| *p > 0) else {
        return Ok(None);
    };
    // SAFETY: `proc_bsdshortinfo` is plain data, and an all-zero value is
    // valid.
    let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdshortinfo>() as libc::c_int;
    // SAFETY: the buffer is a zeroed `proc_bsdshortinfo` of `size` bytes.
    let n = unsafe {
        libc::proc_pidinfo(
            os,
            libc::PROC_PIDT_SHORTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdshortinfo).cast(),
            size,
        )
    };
    Ok((n == size).then_some(info.pbsi_ppid))
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
