//! The concern service for the code of macOS and Linux.

use std::io;
use std::path::{Path, PathBuf};

pub fn write_definition(path: &Path, text: &str) -> io::Result<()> {
    std::fs::write(path, text)
}

pub fn os_user_home() -> io::Result<PathBuf> {
    crate::unix::path::passwd_home()?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "the passwd database has no home folder for this user",
        )
    })
}
