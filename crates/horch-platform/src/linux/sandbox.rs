//! The concern sandbox for Linux: bubblewrap (`bwrap`), when it is on the
//! `PATH` of the command.
//!
//! The whole disk is bound read-only. `/dev` and `/proc` are new, `/tmp` is
//! a new empty tmpfs, and each writable path is bound writable on top of
//! them. `--new-session` takes the controlling terminal away, so a command
//! cannot type into the terminal outside the sandbox (`TIOCSTI`).

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::NotSupported;

/// The first executable `bwrap` in the absolute folders of `path`.
pub(crate) fn program(path: Option<&OsStr>) -> Result<PathBuf, NotSupported> {
    find_bwrap(path).ok_or(
        NotSupported::new("sandbox", "linux").because("bwrap is not on PATH; install bubblewrap"),
    )
}

/// The mounts of `bwrap`: the disk read-only, then each writable path.
pub(crate) fn args(writable: &[PathBuf], temp: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--tmpfs",
        "/tmp",
        "--new-session",
    ]
    .map(OsString::from)
    .to_vec();
    // The tmpfs is the temp folder when it is /tmp, so /tmp is not bound.
    let temp = (temp != Path::new("/tmp")).then_some(temp);
    for path in writable.iter().map(PathBuf::as_path).chain(temp) {
        args.extend([OsString::from("--bind"), path.into(), path.into()]);
    }
    args
}

/// A relative folder of `path` is skipped: it names a folder of the
/// caller, not of the system.
fn find_bwrap(path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("bwrap"))
        .find(|file| {
            file.metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}
