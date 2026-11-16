//! The concern fs for the code of macOS and Linux.

use std::ffi::{CStr, CString};
use std::fs::{DirBuilder, Metadata, OpenOptions};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::Path;

use crate::fs::OwnerAndMode;
use crate::NotSupported;

pub(crate) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

pub(crate) fn create_mode(options: &mut OpenOptions, mode: u32) -> Result<(), NotSupported> {
    options.mode(mode);
    Ok(())
}

pub(crate) fn create_dir_mode(builder: &mut DirBuilder, mode: u32) -> Result<(), NotSupported> {
    builder.mode(mode);
    Ok(())
}

pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    let f = std::fs::File::open(dir)?;
    f.sync_all()
}

pub(crate) fn open_no_block(options: &mut OpenOptions) -> Result<(), NotSupported> {
    options.custom_flags(libc::O_NONBLOCK);
    Ok(())
}

/// [`crate::fs::rename_noreplace`] with `rename`, the call of the OS that
/// fails when the target exists. `rename` returns 0 or sets `errno`.
pub(crate) fn rename_noreplace(
    from: &Path,
    to: &Path,
    rename: fn(&CStr, &CStr) -> libc::c_int,
) -> io::Result<()> {
    let from_c = CString::new(from.as_os_str().as_bytes())?;
    let to_c = CString::new(to.as_os_str().as_bytes())?;
    if rename(&from_c, &to_c) == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    // A file system without the flag: the plain rename below.
    if !matches!(
        e.raw_os_error(),
        Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP)
    ) {
        return Err(e);
    }
    std::fs::rename(from, to)
}

pub(crate) fn owner_and_mode(meta: &Metadata) -> Result<OwnerAndMode, NotSupported> {
    Ok(OwnerAndMode {
        uid: meta.uid(),
        mode: meta.mode(),
    })
}

pub(crate) fn mode_of(meta: &Metadata) -> Result<u32, NotSupported> {
    Ok(meta.permissions().mode())
}

pub(crate) fn current_uid() -> Result<u32, NotSupported> {
    // SAFETY: getuid has no preconditions and cannot fail.
    Ok(unsafe { libc::getuid() })
}

pub(crate) fn is_executable(meta: &Metadata) -> bool {
    meta.is_file() && meta.permissions().mode() & 0o111 != 0
}

pub(crate) fn make_executable(path: &Path) -> io::Result<()> {
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
}

pub(crate) fn file_identity(meta: &Metadata) -> (u64, u64) {
    (meta.dev(), meta.ino())
}

/// [`crate::fs::BreakerGuard`]: the open dir that holds the `flock`, or
/// nothing where `flock` is not available.
#[derive(Debug)]
pub(crate) struct BreakerGuard {
    dir: Option<std::fs::File>,
}

impl BreakerGuard {
    pub(crate) fn take(lock: &Path) -> Option<BreakerGuard> {
        match lock.parent().and_then(|d| std::fs::File::open(d).ok()) {
            // SAFETY: `dir` is an open directory `File` that outlives the call, so its raw fd is valid;
            // `flock` touches no memory.
            Some(dir) => match unsafe {
                libc::flock(dir.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB)
            } {
                0 => Some(BreakerGuard { dir: Some(dir) }),
                _ if io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) => None,
                _ => Some(BreakerGuard { dir: None }),
            },
            None => Some(BreakerGuard { dir: None }),
        }
    }
}

impl Drop for BreakerGuard {
    /// Unlock before the close. A child forked by another thread shares the
    /// open file until it execs, and a close alone leaves the `flock` held
    /// while any copy is open.
    fn drop(&mut self) {
        if let Some(dir) = &self.dir {
            // SAFETY: `dir` is the open directory `File` this guard owns, so its raw fd is valid until drop;
            // `flock` touches no memory.
            unsafe { libc::flock(dir.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}
