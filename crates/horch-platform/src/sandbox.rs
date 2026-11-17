//! The concern sandbox: the check sandbox (SR-4), the sandbox host of PL-2.
//!
//! [`sandboxed`] wraps a command so that it can read the whole disk but
//! write only in a list of paths and in its temp folder:
//!
//! - macOS: `/usr/bin/sandbox-exec` with a generated Seatbelt profile.
//! - Linux: `bwrap` (bubblewrap), when it is on the `PATH` of the command.
//! - Windows: [`NotSupported`]. A check never runs there without its
//!   sandbox: the caller decides what to do (PL-3).
//!
//! The OS module gives the sandbox host and its arguments. This file puts
//! the command after them, so each OS keeps the program, arguments,
//! environment and folder of the command in the same way.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::os::sandbox as os;
use crate::NotSupported;

/// `cmd` in the check sandbox: it can write only in the paths of
/// `writable`, in its temp folder ([`std::env::temp_dir`]) and in
/// `/dev/null`, and it can read the rest of the disk.
///
/// The returned command keeps the program, the arguments, the environment
/// changes and the current folder of `cmd`. `Command` does not show the
/// rest of its settings, so set stdio, `env_clear` and the OS extensions
/// on the returned command. Give absolute paths in `writable`.
///
/// It gives [`NotSupported`] with a reason when this host has no sandbox:
/// on Windows, on Linux when `bwrap` is not on the `PATH` of `cmd`, and on
/// macOS when `/usr/bin/sandbox-exec` is missing.
pub fn sandboxed(cmd: Command, writable: &[PathBuf]) -> Result<Command, NotSupported> {
    let writable: Vec<PathBuf> = writable.iter().map(|path| resolved(path)).collect();
    let temp = resolved(&std::env::temp_dir());
    let host = os::host(&writable, &temp, path_of(&cmd).as_deref())?;
    Ok(wrap(host, &cmd))
}

/// The sandbox host of an OS: the program and the arguments that come
/// before the program of the sandboxed command. Windows has none, so it
/// never builds one.
#[cfg_attr(windows, allow(dead_code))]
pub(crate) struct Host {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<OsString>,
}

/// The `PATH` that `cmd` runs with: its own when it sets or removes one,
/// else the `PATH` of this process.
fn path_of(cmd: &Command) -> Option<OsString> {
    match cmd.get_envs().find(|(key, _)| *key == "PATH") {
        Some((_, value)) => value.map(OsString::from),
        None => std::env::var_os("PATH"),
    }
}

fn wrap(host: Host, cmd: &Command) -> Command {
    let mut sandboxed = Command::new(&host.program);
    sandboxed
        .args(&host.args)
        .arg(cmd.get_program())
        .args(cmd.get_args());
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => sandboxed.env(key, value),
            None => sandboxed.env_remove(key),
        };
    }
    if let Some(dir) = cmd.get_current_dir() {
        sandboxed.current_dir(dir);
    }
    sandboxed
}

/// `path` as the kernel sees it, with each link resolved, or `path` itself
/// when it does not exist yet. Seatbelt compares the resolved path: on
/// macOS `/tmp` is `/private/tmp`.
fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
