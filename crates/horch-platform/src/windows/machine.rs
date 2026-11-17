//! The concern machine for Windows. Windows has no machine probe today:
//! each reading is `NotSupported`
//! (`docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
//! row "Other", and row S12).

use std::path::Path;

use crate::machine::{Disk, Gpu, Memory, Run};
use crate::NotSupported;

const GAP: NotSupported = NotSupported::new("machine", "windows");

pub(crate) fn load_average() -> Result<Option<f64>, NotSupported> {
    Err(GAP)
}

/// No gethostname here: the caller reads `COMPUTERNAME`.
pub(crate) fn host_name() -> Result<Option<String>, NotSupported> {
    Err(GAP)
}

pub(crate) fn memory(_run: Run<'_>) -> Result<Memory, NotSupported> {
    Err(GAP)
}

pub(crate) fn disk(_path: &Path) -> Result<Option<Disk>, NotSupported> {
    Err(GAP)
}

pub(crate) fn max_processes() -> Result<Option<u64>, NotSupported> {
    Err(GAP)
}

pub(crate) fn max_open_files() -> Result<Option<u64>, NotSupported> {
    Err(GAP)
}

pub(crate) fn kernel_free_memory() -> Result<Option<u64>, NotSupported> {
    Err(GAP)
}

pub(crate) fn system_open_file_limit() -> Result<Option<u64>, NotSupported> {
    Err(GAP)
}

/// The caller uses `std::env::consts::ARCH`, as today.
pub(crate) fn uname_arch(_run: Run<'_>) -> Result<Option<String>, NotSupported> {
    Err(GAP)
}

pub(crate) fn gpu(_run: Run<'_>, _arch: &str) -> Result<Gpu, NotSupported> {
    Err(GAP)
}

pub(crate) fn user_process_count() -> Result<Option<u64>, NotSupported> {
    Err(GAP)
}

pub(crate) fn user_open_file_count() -> Result<Option<u64>, NotSupported> {
    Err(GAP)
}
