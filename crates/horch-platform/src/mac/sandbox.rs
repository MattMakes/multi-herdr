//! The concern sandbox for macOS: `sandbox-exec` with a Seatbelt profile.
//!
//! The profile allows all, then denies each file write, then allows a write
//! below each writable path and to `/dev/null`. Last, it denies each file
//! read, file write and Unix socket connection below each hidden path, so a
//! hidden path wins over a writable path. With no network it denies each
//! network act. The paths reach the profile as
//! parameters (`-D W<n>=<path>`, `-D H<n>=<path>`), so a path needs no
//! quoting in it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::sandbox::{Network, SANDBOX_EXEC};
use crate::NotSupported;

/// [`SANDBOX_EXEC`]. Its place is fixed, so `PATH` plays no part.
pub(crate) fn program(_path: Option<&OsStr>) -> Result<PathBuf, NotSupported> {
    if !Path::new(SANDBOX_EXEC).is_file() {
        return Err(
            NotSupported::new("sandbox", "macos").because("/usr/bin/sandbox-exec is missing")
        );
    }
    Ok(PathBuf::from(SANDBOX_EXEC))
}

/// The `-D` parameters and the `-p` profile of `sandbox-exec`.
pub(crate) fn args(
    writable: &[PathBuf],
    temp: Option<&Path>,
    hidden: &[PathBuf],
    network: Network,
) -> Vec<OsString> {
    let mut profile = String::from(
        "(version 1)(allow default)(deny file-write*)(allow file-write* (literal \"/dev/null\")",
    );
    let mut args = Vec::new();
    let mut define = |name: String, path: &Path| {
        let mut value = OsString::from(format!("{name}="));
        value.push(path);
        args.extend([OsString::from("-D"), value]);
    };
    for (n, path) in writable
        .iter()
        .map(PathBuf::as_path)
        .chain(temp)
        .enumerate()
    {
        profile.push_str(&format!(" (subpath (param \"W{n}\"))"));
        define(format!("W{n}"), path);
    }
    profile.push(')');
    for (n, path) in hidden.iter().enumerate() {
        profile.push_str(&format!(
            "(deny file-read* file-write* (subpath (param \"H{n}\")))\
             (deny network-outbound (remote unix-socket (subpath (param \"H{n}\"))))"
        ));
        define(format!("H{n}"), path);
    }
    if network == Network::Denied {
        profile.push_str("(deny network*)");
    }
    args.extend([OsString::from("-p"), OsString::from(profile)]);
    args
}
