//! The concern machine: the machine probe. It reads load, memory, disk, the
//! process limits, the host name, the architecture and the GPU class, with
//! the sources of `horch-core/src/runtime/machine.rs`, and the count of
//! processes and of open files of the user (OR-5):
//!
//! - macOS: `getloadavg`, `gethostname`, `statvfs`, `getrlimit`, and the
//!   commands `uname -m`, `sysctl` and `vm_stat`. The counts read
//!   `proc_listallpids`, `proc_pidinfo(PROC_PIDTBSDINFO)` for the real uid
//!   of each process, and `proc_pidinfo(PROC_PIDLISTFDS)` for the open
//!   files of each process of the user. The count of open files is per
//!   process of the user, not the system count `kern.num_files`. The read
//!   of the uid fails for a zombie, so the count of processes does not
//!   include zombies.
//! - Linux: `getloadavg`, `gethostname`, `statvfs`, `getrlimit`,
//!   `/proc/meminfo`, and the commands `uname -m` and `nvidia-smi -L`. The
//!   counts read the `Uid:` line of `/proc/<pid>/status` and the entries
//!   of `/proc/<pid>/fd`.
//! - Windows: no reading. Each function gives [`NotSupported`]
//!   (`docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
//!   row "Other", and row S12). Core code reports it as unknown (OR-5).
//!
//! The caller runs the probe commands through a [`Run`] function, so the
//! command paths, the timeout and the environment of the child stay in core
//! code. A reading that fails is `None`, as today.
//!
//! The health guards read 4 functions: [`user_process_count`] with the
//! limit [`max_processes`], and [`user_open_file_count`] with the limit
//! [`max_open_files`]. A count is fast: on an Apple M-series Mac with
//! about 700 processes, 1 count takes less than 100 ms.

use std::path::Path;

use crate::os::machine as os;
use crate::NotSupported;

/// A command of the probe. The caller maps it to a program path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// `uname`, for the architecture.
    Uname,
    /// `sysctl` (macOS), for the memory size and the CPU brand.
    Sysctl,
    /// `vm_stat` (macOS), for the available memory.
    VmStat,
    /// `nvidia-smi` (Linux), for the GPU count.
    NvidiaSmi,
}

/// Runs a probe command with its arguments. It gives stdout on exit code 0,
/// and `None` on any failure.
pub type Run<'a> = &'a dyn Fn(Tool, &[&str]) -> Option<String>;

/// The memory of the machine, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub total: Option<u64>,
    pub available: Option<u64>,
}

/// The disk that holds a path, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disk {
    /// The space that the user can use (`f_bavail`).
    pub free: u64,
    pub total: u64,
}

/// The GPU class of the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gpu {
    /// macOS: `uname -m` is `arm64` and the CPU brand starts with `Apple`.
    AppleSilicon,
    /// Linux: `nvidia-smi -L` lists `count` GPUs.
    Nvidia { count: u32 },
    /// No GPU that the probe knows. macOS: the CPU brand is another one.
    /// Linux: `nvidia-smi -L` lists no GPU, or fails, as today.
    None,
    /// macOS only: the `sysctl` of the CPU brand failed.
    Unknown,
}

/// The 1 minute load average, or `None` when `getloadavg` fails.
pub fn load_average() -> Result<Option<f64>, NotSupported> {
    os::load_average()
}

/// The host name from `gethostname`, as the OS gives it: not trimmed, and
/// possibly empty. `None` when the call fails. `runtime/machine.rs` trims
/// it and drops an empty name; `fsx.rs` uses it as it is.
pub fn host_name() -> Result<Option<String>, NotSupported> {
    os::host_name()
}

/// The memory size and the available memory.
pub fn memory(run: Run<'_>) -> Result<Memory, NotSupported> {
    os::memory(run)
}

/// The free and total space of the disk that holds `path`, or `None` when
/// `statvfs` fails.
pub fn disk(path: &Path) -> Result<Option<Disk>, NotSupported> {
    os::disk(path)
}

/// The soft limit of processes of the user (`RLIMIT_NPROC`). `None` when it
/// is infinite or unknown.
pub fn max_processes() -> Result<Option<u64>, NotSupported> {
    os::max_processes()
}

/// The soft limit of open files (`RLIMIT_NOFILE`). `None` when it is
/// infinite or unknown.
pub fn max_open_files() -> Result<Option<u64>, NotSupported> {
    os::max_open_files()
}

/// The count of processes of the user: the processes with the real uid of
/// this process whose uid the probe can read. A process that exits during
/// the read is not counted. On macOS a zombie is not counted, but
/// `RLIMIT_NPROC` ([`max_processes`]) counts it, so the count can be lower
/// than the count of the limit. `None` when the process list cannot be read.
pub fn user_process_count() -> Result<Option<u64>, NotSupported> {
    os::user_process_count()
}

/// The count of open files of all the processes of the user (see
/// [`user_process_count`]). A process whose files cannot be read adds 0.
/// `None` when the process list cannot be read.
pub fn user_open_file_count() -> Result<Option<u64>, NotSupported> {
    os::user_open_file_count()
}

/// The output of `uname -m`, trimmed, or `None` when it fails or is empty.
/// The caller uses `std::env::consts::ARCH` in place of `None`.
pub fn uname_arch(run: Run<'_>) -> Result<Option<String>, NotSupported> {
    os::uname_arch(run)
}

/// The GPU class. `arch` is the architecture that [`uname_arch`] gave.
pub fn gpu(run: Run<'_>, arch: &str) -> Result<Gpu, NotSupported> {
    os::gpu(run, arch)
}

/// Parse `vm_stat` output: (free + inactive + speculative pages) x page size.
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

/// Parse one `/proc/meminfo` key in kB and return bytes.
pub fn parse_meminfo(text: &str, key: &str) -> Option<u64> {
    let rest = text
        .lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?;
    let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
    Some(kb * 1024)
}
