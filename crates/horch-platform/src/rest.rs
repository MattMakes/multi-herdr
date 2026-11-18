//! The concern rest: the small facts and actions that differ by OS.
//!
//! - The release archive: [`RELEASE_ARCHIVE_SUFFIX`] and [`extract_release`]
//!   (`horch/src/cmd/selfupdate.rs`).
//! - The self-update restart: [`replace_process`].
//! - The user PATH: [`ensure_user_path`] and [`USER_PATH_IN_REGISTRY`]
//!   (`herdr-install`), [`path_help`] (`horch install`).
//! - The executable name and extensions: [`exe_name`] and
//!   [`path_extensions`].
//! - The install facts: [`RUNNING_EXE_IS_LOCKED`] and [`SHELL_LAUNCHERS`].
//! - The host tools line of the project notes: [`HOST_TOOLS`].
//! - The Prime status fact: [`STATUS_HAS_START_TIMES`].
//!
//! File identity is in the concern `fs`. The sandbox host has no OS `cfg`
//! today (`docs/tickets/fleetd-pp/MAP.md`, Q5).

use std::fmt;
use std::io;
use std::path::Path;

use crate::os::rest as os;
use crate::start::{Start, TOKEN};
use crate::NotSupported;

/// The file extension of the release archive of this OS: `tar.gz`, or
/// `zip` on Windows.
pub const RELEASE_ARCHIVE_SUFFIX: &str = os::RELEASE_ARCHIVE_SUFFIX;

/// Whether the user PATH is in the Windows registry, so the installer
/// updates it with [`ensure_user_path`]. When it is `false`, the installer
/// tells the user to change the shell config.
pub const USER_PATH_IN_REGISTRY: bool = os::USER_PATH_IN_REGISTRY;

/// Whether the OS locks the file of a running executable. Then an install
/// moves the old file to its `.old` path first, and a later install removes
/// the `.old` files.
pub const RUNNING_EXE_IS_LOCKED: bool = os::RUNNING_EXE_IS_LOCKED;

/// Whether the shell launchers `herdr-fleet` and `herdr-run` work on this
/// OS. Windows has none: `horch fleet` is the entry point there.
pub const SHELL_LAUNCHERS: bool = os::SHELL_LAUNCHERS;

/// The host tools line of the project notes.
pub const HOST_TOOLS: &str = os::HOST_TOOLS;

/// Whether the Prime status gives a start time with a pid. On Windows it
/// does not: the pid is all there is.
pub const STATUS_HAS_START_TIMES: bool = os::STATUS_HAS_START_TIMES;

/// Unpacks a release archive of [`RELEASE_ARCHIVE_SUFFIX`] into `dir`. The
/// archive is flat: an entry name with a separator or `..`, or an entry that
/// is not a file, is refused. On unix each file gets mode `0o755`.
pub fn extract_release(archive: &[u8], dir: &Path) -> io::Result<()> {
    os::extract_release(archive, dir)
}

/// Replaces this process with `cmd`. On unix it calls `exec`. On Windows it
/// starts `cmd`, waits, and exits with its code, or 1 with no code. It
/// returns only the error of a start that failed.
pub fn replace_process(cmd: impl Start) -> io::Error {
    os::replace_process(cmd.into_std(TOKEN))
}

/// Puts `dir` first in the user PATH of the registry (`HKCU\Environment`),
/// and gives `true` when it changed the PATH. `prepend` gives the new PATH
/// from the current one and `dir`, or `None` when `dir` is already first.
/// macOS and Linux have no user PATH to change: `NotSupported`
/// (`docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
/// row "Other").
pub fn ensure_user_path(
    dir: &Path,
    prepend: &dyn Fn(&str, &str) -> Option<String>,
) -> Result<io::Result<bool>, NotSupported> {
    os::ensure_user_path(dir, prepend)
}

/// The shell lines that put `dir` on the PATH: PowerShell lines on Windows,
/// a shell config line elsewhere.
pub fn path_help(dir: &Path) -> String {
    os::path_help(dir)
}

/// The file name of the executable `stem` on this OS.
pub fn exe_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// Executable extensions to try when a name has none: the `PATHEXT` entries
/// on Windows (lowercased), and nothing anywhere else. The caller passes
/// the value of `PATHEXT`.
pub fn path_extensions(pathext: Option<&str>) -> Vec<String> {
    os::path_extensions(pathext)
}

/// A release archive is flat: an entry name with a separator or `..` is
/// refused.
pub(crate) fn flat_name(name: &str) -> io::Result<&str> {
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the release archive holds an unexpected entry {name:?}"),
        ));
    }
    Ok(name)
}

/// The entry `name` is not a file.
pub(crate) fn not_a_file(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("the release archive entry {name:?} is not a file"),
    )
}

/// An error with the message of today and its cause as `source`, as
/// `anyhow::Context` gives it. The caller sees the same text with `{}` and
/// with `{:#}`.
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

/// Wraps `source` in an `io::Error` with the text `message`.
pub(crate) fn context(
    message: impl Into<String>,
    source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> io::Error {
    io::Error::other(Context {
        message: message.into(),
        source: source.into(),
    })
}
