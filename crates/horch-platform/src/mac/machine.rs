//! The concern machine for macOS.

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
