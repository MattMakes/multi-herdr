use std::fmt;
use std::io;
use std::path::Path;

use crate::os::rest as os;
use crate::start::{Start, TOKEN};
use crate::NotSupported;

pub const RELEASE_ARCHIVE_SUFFIX: &str = os::RELEASE_ARCHIVE_SUFFIX;

pub const USER_PATH_IN_REGISTRY: bool = os::USER_PATH_IN_REGISTRY;

pub const RUNNING_EXE_IS_LOCKED: bool = os::RUNNING_EXE_IS_LOCKED;

pub const SHELL_LAUNCHERS: bool = os::SHELL_LAUNCHERS;

pub const HOST_TOOLS: &str = os::HOST_TOOLS;

pub const STATUS_HAS_START_TIMES: bool = os::STATUS_HAS_START_TIMES;

pub fn extract_release(archive: &[u8], dir: &Path) -> io::Result<()> {
    os::extract_release(archive, dir)
}

pub fn replace_process(cmd: impl Start) -> io::Error {
    os::replace_process(cmd.into_std(TOKEN))
}

pub fn ensure_user_path(
    dir: &Path,
    prepend: &dyn Fn(&str, &str) -> Option<String>,
) -> Result<io::Result<bool>, NotSupported> {
    os::ensure_user_path(dir, prepend)
}

pub fn path_help(dir: &Path) -> String {
    os::path_help(dir)
}

pub fn exe_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

pub fn path_extensions(pathext: Option<&str>) -> Vec<String> {
    os::path_extensions(pathext)
}

pub(crate) fn flat_name(name: &str) -> io::Result<&str> {
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the release archive holds an unexpected entry {name:?}"),
        ));
    }
    Ok(name)
}

pub(crate) fn not_a_file(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("the release archive entry {name:?} is not a file"),
    )
}

#[derive(Debug)]
struct Context {
    message: String,
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl fmt::Display for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Context {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.source)
    }
}

pub(crate) fn context(
    message: impl Into<String>,
    source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> io::Error {
    io::Error::other(Context {
        message: message.into(),
        source: source.into(),
    })
}
