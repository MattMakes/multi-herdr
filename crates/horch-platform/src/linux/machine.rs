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
