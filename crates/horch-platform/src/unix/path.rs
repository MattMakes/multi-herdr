//! The concern path for the code of macOS and Linux.

use std::fs::Metadata;
use std::path::{Path, PathBuf};

use crate::NotSupported;

pub const HOME_VAR: &str = "HOME";
pub const CONFIG_BASE_VAR: Option<&str> = None;
pub const INSTALL_BASE_VAR: Option<&str> = None;

pub fn passwd_home() -> Result<Option<PathBuf>, NotSupported> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: `passwd` is plain data; all-zero bytes are a valid value.
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut out: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is valid for the call, and `buf.len()` is the
    // size of `buf`. The strings in `pw` point into `buf`, which outlives them.
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut pw,
            buf.as_mut_ptr(),
            buf.len(),
            &mut out,
        )
    };
    if rc != 0 || out.is_null() || pw.pw_dir.is_null() {
        return Ok(None);
    }
    // SAFETY: getpwuid_r succeeded, so `pw_dir` is a NUL-terminated string in `buf`.
    let dir = unsafe { CStr::from_ptr(pw.pw_dir) };
    Ok(Some(PathBuf::from(OsStr::from_bytes(dir.to_bytes()))))
}

pub fn symlink(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

pub fn copy_symlink(from: &Path, to: &Path) -> std::io::Result<()> {
    let target = std::fs::read_link(from)?;
    std::os::unix::fs::symlink(&target, to)
}

pub fn private_link_home_supported() -> bool {
    true
}

fn no_junctions() -> NotSupported {
    NotSupported {
        concern: "path",
        os: std::env::consts::OS,
    }
}

pub fn is_reparse_point(_meta: &Metadata) -> Result<bool, NotSupported> {
    Err(no_junctions())
}

pub fn set_junction(_link: &Path, _target: &Path) -> std::io::Result<()> {
    Err(no_junctions().into())
}

pub fn clear_link(_link: &Path) -> std::io::Result<()> {
    Err(no_junctions().into())
}
