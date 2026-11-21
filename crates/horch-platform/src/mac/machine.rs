use std::path::Path;

use crate::machine::{parse_vm_stat, Disk, Gpu, Memory, Run, Tool};
use crate::unix::machine as unix;
use crate::NotSupported;

pub(crate) fn load_average() -> Result<Option<f64>, NotSupported> {
    Ok(unix::load_average())
}

pub(crate) fn host_name() -> Result<Option<String>, NotSupported> {
    Ok(unix::host_name())
}

pub(crate) fn memory(run: Run<'_>) -> Result<Memory, NotSupported> {
    let total = run(Tool::Sysctl, &["-n", "hw.memsize"]).and_then(|s| s.trim().parse().ok());
    let available = run(Tool::VmStat, &[]).and_then(|s| parse_vm_stat(&s));
    Ok(Memory { total, available })
}

pub(crate) fn disk(path: &Path) -> Result<Option<Disk>, NotSupported> {
    Ok(unix::disk(path))
}

pub(crate) fn max_processes() -> Result<Option<u64>, NotSupported> {
    Ok(unix::max_processes())
}

pub(crate) fn max_open_files() -> Result<Option<u64>, NotSupported> {
    Ok(unix::max_open_files())
}

pub(crate) fn kernel_free_memory() -> Result<Option<u64>, NotSupported> {
    let percent =
        sysctl::<libc::c_int>(c"kern.memorystatus_level").filter(|p| (0..=100).contains(p));
    let total = sysctl::<u64>(c"hw.memsize");
    Ok(percent.zip(total).map(|(p, total)| total / 100 * p as u64))
}

pub(crate) fn system_open_file_limit() -> Result<Option<u64>, NotSupported> {
    Ok(sysctl::<libc::c_int>(c"kern.maxfiles")
        .filter(|v| *v > 0)
        .map(|v| v as u64))
}

pub(crate) fn system_open_file_count() -> Result<Option<u64>, NotSupported> {
    Ok(sysctl::<libc::c_int>(c"kern.num_files")
        .filter(|v| *v >= 0)
        .map(|v| v as u64))
}

fn sysctl<T: Copy + Default>(name: &std::ffi::CStr) -> Option<T> {
    let mut value = T::default();
    let mut size = size_of::<T>();
    // SAFETY: the name is a NUL-terminated string, and `value` is an
    // out-parameter of `size` bytes that the call writes at most.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut T).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && size == size_of::<T>()).then_some(value)
}

pub(crate) fn uname_arch(run: Run<'_>) -> Result<Option<String>, NotSupported> {
    Ok(unix::uname_arch(run))
}

pub(crate) fn gpu(run: Run<'_>, arch: &str) -> Result<Gpu, NotSupported> {
    Ok(
        match run(Tool::Sysctl, &["-n", "machdep.cpu.brand_string"]) {
            Some(b) if arch == "arm64" && b.trim_start().starts_with("Apple") => Gpu::AppleSilicon,
            Some(_) => Gpu::None,
            None => Gpu::Unknown,
        },
    )
}

pub(crate) fn user_process_count() -> Result<Option<u64>, NotSupported> {
    Ok(user_pids().map(|pids| pids.len() as u64))
}

pub(crate) fn user_open_file_count() -> Result<Option<u64>, NotSupported> {
    Ok(user_pids().map(|pids| pids.into_iter().map(open_file_count).sum()))
}

fn user_pids() -> Option<Vec<libc::pid_t>> {
    // SAFETY: a null buffer of size 0 asks only for the count of pids.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return None;
    }
    let mut pids: Vec<libc::pid_t> = vec![0; count as usize + 64];
    let bytes = (pids.len() * size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: `pids` holds `bytes` bytes, and the call writes at most that.
    let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if count <= 0 {
        return None;
    }
    pids.truncate(count as usize);
    // SAFETY: getuid has no precondition and cannot fail.
    let uid = unsafe { libc::getuid() };
    pids.retain(|&pid| real_uid(pid) == Some(uid));
    Some(pids)
}

fn real_uid(pid: libc::pid_t) -> Option<libc::uid_t> {
    let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: `info` is a zeroed out-parameter of `size` bytes that
    // `proc_pidinfo` fills in, and the call writes at most `size` bytes.
    let (written, info) = unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let written = libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        );
        (written, info)
    };
    (written == size).then_some(info.pbi_ruid)
}

fn open_file_count(pid: libc::pid_t) -> u64 {
    // SAFETY: a null buffer of size 0 asks only for the size of the list.
    let bytes =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    if bytes <= 0 {
        return 0;
    }
    let mut fds: Vec<libc::proc_fdinfo> =
        Vec::with_capacity(bytes as usize / libc::PROC_PIDLISTFD_SIZE as usize);
    let bytes = (fds.capacity() * libc::PROC_PIDLISTFD_SIZE as usize) as libc::c_int;
    // SAFETY: `fds` has room for `bytes` bytes, and the call writes at most
    // that. The entries stay unread, so they need no initialization.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            bytes,
        )
    };
    if written <= 0 {
        return 0;
    }
    (written / libc::PROC_PIDLISTFD_SIZE) as u64
}
