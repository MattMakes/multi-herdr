//! The concern fs for Windows. Each gap is documented in `crate::fs`.

use std::fs::{DirBuilder, Metadata, OpenOptions};
use std::io;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use crate::fs::OwnerAndMode;
use crate::NotSupported;

const NOT_SUPPORTED: NotSupported = NotSupported::new("fs", "windows");

pub(crate) fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Err(NOT_SUPPORTED.into())
}

pub(crate) fn create_mode(_options: &mut OpenOptions, _mode: u32) -> Result<(), NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn create_dir_mode(_builder: &mut DirBuilder, _mode: u32) -> Result<(), NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Err(NOT_SUPPORTED.into())
}

pub(crate) fn open_no_block(_options: &mut OpenOptions) -> Result<(), NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn remove_dir_below(_root: &Path, _names: &[&str]) -> io::Result<()> {
    Err(NOT_SUPPORTED.into())
}

pub(crate) fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

pub(crate) fn owner_and_mode(_meta: &Metadata) -> Result<OwnerAndMode, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn mode_of(_meta: &Metadata) -> Result<u32, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn current_uid() -> Result<u32, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn is_executable(meta: &Metadata) -> bool {
    meta.is_file()
}

pub(crate) fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

pub(crate) fn file_identity(meta: &Metadata) -> (u64, u64) {
    (0, meta.creation_time())
}

/// [`crate::fs::BreakerGuard`]: it holds nothing on Windows.
#[derive(Debug)]
pub(crate) struct BreakerGuard;

impl BreakerGuard {
    pub(crate) fn take(_lock: &Path) -> Option<BreakerGuard> {
        Some(BreakerGuard)
    }
}
