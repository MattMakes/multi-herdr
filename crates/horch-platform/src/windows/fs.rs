//! The concern fs for Windows. Each gap is documented in `crate::fs`.

use std::fs::{DirBuilder, File, Metadata, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
};

use crate::channel::User;
use crate::fs::OwnerAndMode;
use crate::windows::channel::sid_text;
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

pub(crate) fn open_no_follow(path: &Path) -> io::Result<File> {
    // `FILE_FLAG_OPEN_REPARSE_POINT` opens a link itself, not its target.
    // `FILE_FLAG_BACKUP_SEMANTICS` lets the call open a folder too.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is a reparse point", path.display()),
        ));
    }
    Ok(file)
}

pub(crate) fn owner_of(file: &File) -> io::Result<User> {
    let mut owner: PSID = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle of `file` is open for the call, and `owner` and
    // `descriptor` are valid for 1 write each. The other outputs are null,
    // because the call does not ask for them.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    // SAFETY: `owner` points into `descriptor`, which lives until the
    // LocalFree below.
    let sid = unsafe { sid_text(owner) };
    // SAFETY: the system allocated `descriptor` with LocalAlloc, and
    // nothing reads it or `owner` after this call.
    unsafe { LocalFree(descriptor) };
    sid.map(User::Sid)
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
