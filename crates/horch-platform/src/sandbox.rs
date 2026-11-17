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
//! The OS module gives the sandbox host program and its arguments. This file puts
//! the command after them, so each OS keeps the program, arguments,
//! environment and folder of the command in the same way.

use std::ffi::{OsStr, OsString};
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
    let program = os::program(path_of(&cmd).as_deref())?;
    let writable: Vec<PathBuf> = writable.iter().map(|path| resolved(path)).collect();
    let args = os::args(&writable, &resolved(&std::env::temp_dir()));
    Ok(wrap(&program, &args, &cmd))
}

/// Why this host has no sandbox for a command that runs with `path` as its
/// `PATH`, or `None` when it has one. [`sandboxed`] gives the same
/// [`NotSupported`] value. Each caller that needs the sandbox host asks
/// here: macOS needs `/usr/bin/sandbox-exec`, and Linux needs `bwrap` in an
/// absolute folder of `path`.
pub fn host_problem(path: Option<&OsStr>) -> Option<NotSupported> {
    os::program(path).err()
}

/// The `PATH` that `cmd` runs with: its own when it sets or removes one,
/// else the `PATH` of this process.
fn path_of(cmd: &Command) -> Option<OsString> {
    match cmd.get_envs().find(|(key, _)| *key == "PATH") {
        Some((_, value)) => value.map(OsString::from),
        None => std::env::var_os("PATH"),
    }
}

/// A command that runs `program` with `args`, then the program of `cmd`.
fn wrap(program: &Path, args: &[OsString], cmd: &Command) -> Command {
    let mut sandboxed = Command::new(program);
    sandboxed
        .args(args)
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
