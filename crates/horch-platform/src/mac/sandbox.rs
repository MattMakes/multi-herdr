//! The concern sandbox for macOS: `sandbox-exec` with a Seatbelt profile.
//!
//! The profile allows all, then denies each file write, then allows a write
//! below each writable path and to `/dev/null`. The paths reach the profile
//! as parameters (`-D W<n>=<path>`), so a path needs no quoting in it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::sandbox::Host;
use crate::NotSupported;

/// Where macOS keeps the Seatbelt launcher.
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

pub(crate) fn host(
    writable: &[PathBuf],
    temp: &Path,
    _path: Option<&OsStr>,
) -> Result<Host, NotSupported> {
    if !Path::new(SANDBOX_EXEC).is_file() {
        return Err(
            NotSupported::new("sandbox", "macos").because("/usr/bin/sandbox-exec is missing")
        );
    }
    let mut profile = String::from(
        "(version 1)(allow default)(deny file-write*)(allow file-write* (literal \"/dev/null\")",
    );
    let mut args = Vec::new();
    for (n, path) in writable
        .iter()
        .map(PathBuf::as_path)
        .chain([temp])
        .enumerate()
    {
        profile.push_str(&format!(" (subpath (param \"W{n}\"))"));
        let mut define = OsString::from(format!("W{n}="));
        define.push(path);
        args.extend([OsString::from("-D"), define]);
    }
    profile.push(')');
    args.extend([OsString::from("-p"), OsString::from(profile)]);
    Ok(Host {
        program: PathBuf::from(SANDBOX_EXEC),
        args,
    })
}
