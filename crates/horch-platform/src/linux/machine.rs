//! The concern machine for Linux.

use std::path::Path;

use crate::machine::{parse_meminfo, Disk, Gpu, Memory, Run, Tool};
use crate::unix::machine as unix;
use crate::NotSupported;

pub(crate) fn load_average() -> Result<Option<f64>, NotSupported> {
    Ok(unix::load_average())
}

pub(crate) fn host_name() -> Result<Option<String>, NotSupported> {
    Ok(unix::host_name())
}

pub(crate) fn memory(_run: Run<'_>) -> Result<Memory, NotSupported> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok();
    let total = meminfo
        .as_deref()
        .and_then(|t| parse_meminfo(t, "MemTotal"));
    let available = meminfo
        .as_deref()
        .and_then(|t| parse_meminfo(t, "MemAvailable"));
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

/// Linux needs no such reading: `MemAvailable` of [`memory`] already counts the
/// file cache that the kernel can free as available.
pub(crate) fn kernel_free_memory() -> Result<Option<u64>, NotSupported> {
    Err(NotSupported::new("machine", "linux"))
}

/// `/proc/sys/fs/file-max`: the open file limit of the system.
pub(crate) fn system_open_file_limit() -> Result<Option<u64>, NotSupported> {
    Ok(std::fs::read_to_string("/proc/sys/fs/file-max")
        .ok()
        .and_then(|s| s.trim().parse().ok()))
}

/// The first field of `/proc/sys/fs/file-nr`: the count of open files of
/// the system.
pub(crate) fn system_open_file_count() -> Result<Option<u64>, NotSupported> {
    Ok(std::fs::read_to_string("/proc/sys/fs/file-nr")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse().ok()))
}

pub(crate) fn uname_arch(run: Run<'_>) -> Result<Option<String>, NotSupported> {
    Ok(unix::uname_arch(run))
}

pub(crate) fn gpu(run: Run<'_>, _arch: &str) -> Result<Gpu, NotSupported> {
    Ok(match run(Tool::NvidiaSmi, &["-L"]) {
        Some(list) => {
            let count = list.lines().filter(|l| l.starts_with("GPU ")).count() as u32;
            if count > 0 {
                Gpu::Nvidia { count }
            } else {
                Gpu::None
            }
        }
        None => Gpu::None,
    })
}

pub(crate) fn user_process_count() -> Result<Option<u64>, NotSupported> {
    Ok(user_pids().map(|pids| pids.len() as u64))
}

pub(crate) fn user_open_file_count() -> Result<Option<u64>, NotSupported> {
    Ok(user_pids().map(|pids| pids.iter().map(String::as_str).map(open_file_count).sum()))
}

/// The pids of the processes with the real uid of this process, as the
/// names of their `/proc` folders.
fn user_pids() -> Option<Vec<String>> {
    // SAFETY: getuid has no precondition and cannot fail.
    let uid = unsafe { libc::getuid() };
    let entries = std::fs::read_dir("/proc").ok()?;
    Some(
        entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|name| name.bytes().all(|b| b.is_ascii_digit()))
            .filter(|pid| real_uid(pid) == Some(uid))
            .collect(),
    )
}

/// The real uid of a process: the first field of the `Uid:` line of
/// `/proc/<pid>/status`. `None` when the process exited.
fn real_uid(pid: &str) -> Option<libc::uid_t> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// The count of entries of `/proc/<pid>/fd`, or 0 when the read fails.
fn open_file_count(pid: &str) -> u64 {
    std::fs::read_dir(format!("/proc/{pid}/fd")).map_or(0, |fds| fds.count() as u64)
}
