use std::path::Path;

use crate::os::machine as os;
use crate::NotSupported;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Uname,
    Sysctl,
    VmStat,
    NvidiaSmi,
}

pub type Run<'a> = &'a dyn Fn(Tool, &[&str]) -> Option<String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub total: Option<u64>,
    pub available: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disk {
    pub free: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gpu {
    AppleSilicon,
    Nvidia { count: u32 },
    None,
    Unknown,
}

pub fn load_average() -> Result<Option<f64>, NotSupported> {
    os::load_average()
}

pub fn host_name() -> Result<Option<String>, NotSupported> {
    os::host_name()
}

pub fn memory(run: Run<'_>) -> Result<Memory, NotSupported> {
    os::memory(run)
}

pub fn disk(path: &Path) -> Result<Option<Disk>, NotSupported> {
    os::disk(path)
}

pub fn max_processes() -> Result<Option<u64>, NotSupported> {
    os::max_processes()
}

pub fn max_open_files() -> Result<Option<u64>, NotSupported> {
    os::max_open_files()
}

pub fn kernel_free_memory() -> Result<Option<u64>, NotSupported> {
    os::kernel_free_memory()
}

pub fn system_open_file_limit() -> Result<Option<u64>, NotSupported> {
    os::system_open_file_limit()
}

pub fn system_open_file_count() -> Result<Option<u64>, NotSupported> {
    os::system_open_file_count()
}

pub fn user_process_count() -> Result<Option<u64>, NotSupported> {
    os::user_process_count()
}

pub fn user_open_file_count() -> Result<Option<u64>, NotSupported> {
    os::user_open_file_count()
}

pub fn uname_arch(run: Run<'_>) -> Result<Option<String>, NotSupported> {
    os::uname_arch(run)
}

pub fn gpu(run: Run<'_>, arch: &str) -> Result<Gpu, NotSupported> {
    os::gpu(run, arch)
}

pub fn parse_vm_stat(text: &str) -> Option<u64> {
    let header = text.lines().next()?;
    let page_size: u64 = header
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let pages = |key: &str| -> u64 {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|rest| rest.trim().trim_end_matches('.').parse().ok())
            .unwrap_or(0)
    };
    let total = pages("Pages free:") + pages("Pages inactive:") + pages("Pages speculative:");
    Some(total * page_size)
}

pub fn parse_meminfo(text: &str, key: &str) -> Option<u64> {
    let rest = text
        .lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?;
    let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
    Some(kb * 1024)
}
