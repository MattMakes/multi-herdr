//! The concern sandbox for Linux: bubblewrap (`bwrap`), when it is on the
//! `PATH` of the command.
//!
//! The whole disk is bound read-only. `/dev` and `/proc` are new. With a
//! temp folder `/tmp` is a new empty tmpfs. Each writable path is bound
//! writable on top of them. Each hidden folder gets a new empty tmpfs on top of that.
//! `--new-session` takes the controlling terminal away, so a command
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

/// The mounts of `bwrap`: the disk read-only, then each writable path, then
/// each hidden path, so a hidden path wins over a writable path.
///
/// A writable path that does not exist yet is replaced by its nearest
/// existing folder: `bwrap` cannot bind a path that does not exist. A
/// writable path in another writable path is not bound again: a command
/// cannot rename a bound folder. A hidden folder gets a new empty tmpfs,
/// and a hidden file gets `/dev/null`. A hidden path that does not exist is
/// skipped.
pub(crate) fn args(writable: &[PathBuf], temp: Option<&Path>, hidden: &[PathBuf]) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--new-session",
    ]
    .map(OsString::from)
    .to_vec();
    // With a temp folder, /tmp is a new empty tmpfs. With none, /tmp is
    // the read-only /tmp of the disk.
    if temp.is_some() {
        args.extend(["--tmpfs", "/tmp"].map(OsString::from));
    }
    // The tmpfs is the temp folder when it is /tmp, so /tmp is not bound.
    let temp = temp.filter(|temp| *temp != Path::new("/tmp"));
    let binds: Vec<PathBuf> = writable
        .iter()
        .map(|path| existing(path))
        .chain(temp.map(Path::to_path_buf))
        .collect();
    for (n, path) in binds.iter().enumerate() {
        let covered = binds
            .iter()
            .enumerate()
            .any(|(m, other)| path.starts_with(other) && (path != other || m < n));
        if !covered {
            args.extend([OsString::from("--bind"), path.into(), path.into()]);
        }
    }
    for path in hidden {
        if path.is_dir() {
            args.extend([OsString::from("--tmpfs"), path.into()]);
        } else if path.exists() {
            args.extend([OsString::from("--ro-bind"), "/dev/null".into(), path.into()]);
        }
    }
    args
}

/// `path`, or its nearest existing folder when it does not exist.
fn existing(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|p| p.exists())
        .unwrap_or(Path::new("/"))
        .to_path_buf()
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
