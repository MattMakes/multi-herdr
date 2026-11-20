//! The concern sandbox for Linux: bubblewrap (`bwrap`), when it is on the
//! `PATH` of the command.
//!
//! The whole disk is bound read-only. `/dev` and `/proc` are new. With a
//! temp folder `/tmp` is a new empty tmpfs. Then 1 mount for each rule,
//! from the shortest path to the longest: a readable path read-only, a
//! writable path writable, and a new empty tmpfs on a hidden folder. With
//! no network the command gets a new network namespace (`--unshare-net`).
//! `--new-session` takes the controlling terminal away, so a command
//! cannot type into the terminal outside the sandbox (`TIOCSTI`).

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::sandbox::{Network, Rule};
use crate::NotSupported;

/// The first executable `bwrap` in the absolute folders of `path`.
pub(crate) fn program(path: Option<&OsStr>) -> Result<PathBuf, NotSupported> {
    find_bwrap(path).ok_or(
        NotSupported::new("sandbox", "linux").because("bwrap is not on PATH; install bubblewrap"),
    )
}

/// The mounts of `bwrap`: the disk read-only, then 1 mount for each rule,
/// in order, so the mount of a longer path covers the ones before it. A
/// readable path is bound read-only, a writable path is bound writable, a
/// hidden folder gets a new empty tmpfs and a hidden file gets
/// `/dev/null`. A path that does not exist gets no mount. With a temp
/// folder `/tmp` is a new empty tmpfs. With no network the command gets a
/// new network namespace.
pub(crate) fn args(rules: &[Rule<'_>], temp: bool, network: Network) -> Vec<OsString> {
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
    if network != Network::Open {
        args.push("--unshare-net".into());
    }
    if temp {
        args.extend(["--tmpfs", "/tmp"].map(OsString::from));
    }
    for rule in rules {
        match *rule {
            // The tmpfs is the temp folder when it is /tmp.
            Rule::Write(path) if temp && path == Path::new("/tmp") => {}
            Rule::Read(path) | Rule::Write(path) | Rule::Hide(path) if !path.exists() => {}
            Rule::Read(path) => args.extend(["--ro-bind".into(), path.into(), path.into()]),
            Rule::Write(path) => args.extend(["--bind".into(), path.into(), path.into()]),
            Rule::Hide(path) if path.is_dir() => args.extend(["--tmpfs".into(), path.into()]),
            Rule::Hide(path) => {
                args.extend(["--ro-bind".into(), "/dev/null".into(), path.into()]);
            }
        }
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
