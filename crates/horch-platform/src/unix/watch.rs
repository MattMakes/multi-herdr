//! The concern watch for the code of macOS and Linux.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// `dir` as a C string for a call of the OS.
pub(crate) fn c_path(dir: &Path) -> io::Result<CString> {
    CString::new(dir.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}
