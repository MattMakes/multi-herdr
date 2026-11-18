//! The concern sandbox: the check sandbox (SR-4), the sandbox host of PL-2.
//!
//! [`sandboxed`] wraps a command so that it can read the whole disk but
//! write only in a list of paths and in its temp folder.
//! [`sandboxed_hiding`] wraps a command so that it can write only in a list
//! of paths, cannot read, write or connect to a Unix socket below a list of
//! hidden paths, can read again a list of paths in them, and can have no
//! network:
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

/// Where macOS keeps `sandbox-exec`, the Seatbelt launcher of the sandbox.
/// Only macOS uses it.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

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
/// macOS when [`SANDBOX_EXEC`] is missing.
pub fn sandboxed(cmd: Command, writable: &[PathBuf]) -> Result<Command, NotSupported> {
    let temp = resolved(&std::env::temp_dir());
    let reach = Reach {
        writable,
        readable: &[],
        hidden: &[],
        network: Network::Open,
    };
    wrapped(cmd, &reach, Some(&temp))
}

/// The network rule of [`sandboxed_hiding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    /// The command can use the network.
    Open,
    /// The command cannot open a network connection. On macOS this
    /// includes each Unix socket connection. On Linux the command gets a
    /// new network namespace: it has no network and no abstract Unix
    /// socket, but it can connect to a Unix socket file that it can see.
    Denied,
}

/// What a command of [`sandboxed_hiding`] can reach. Give absolute paths.
#[derive(Debug, Clone, Copy)]
pub struct Reach<'a> {
    /// The command can read and write below each of these paths.
    pub writable: &'a [PathBuf],
    /// The command can read, and not write, below each of these paths.
    pub readable: &'a [PathBuf],
    /// The command cannot read, write or connect to a Unix socket below
    /// each of these paths.
    pub hidden: &'a [PathBuf],
    pub network: Network,
}

/// `cmd` in a sandbox with no temp folder. It can write only below the
/// writable paths of `reach` and in `/dev/null`, and it can read the whole
/// disk but the hidden paths.
///
/// For each path the rule of the longest path of `reach` that holds it
/// applies. So a writable or readable path in a hidden path can be reached
/// again, and a hidden path in a writable or readable path is hidden. For
/// 1 path in 2 lists, hidden wins over writable, and writable over
/// readable.
///
/// - macOS: Seatbelt checks each file read and write and each Unix socket
///   connection against the rules, whether the path exists or not.
/// - Linux: the rules are mounts, from the shortest path to the longest.
///   A new empty tmpfs covers each hidden folder, and `/dev/null` covers
///   each hidden file, so a socket file in a hidden folder is gone. A path
///   that does not exist when the command starts gets no mount: a hidden
///   one is not covered, a readable one in a hidden folder is not there,
///   and a writable one is not writable. So give writable paths that
///   exist. An abstract Unix socket has no path: it is not denied (but see
///   [`Network::Denied`]). A bound path is a mount point: the command
///   cannot rename or remove it, only what is in it.
///
/// The rest is as in [`sandboxed`]: the command and its settings, and
/// [`NotSupported`] on a host with no sandbox.
pub fn sandboxed_hiding(cmd: Command, reach: &Reach<'_>) -> Result<Command, NotSupported> {
    wrapped(cmd, reach, None)
}

/// 1 rule of a sandbox on 1 resolved path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule<'a> {
    Read(&'a Path),
    Write(&'a Path),
    Hide(&'a Path),
}

impl Rule<'_> {
    fn key(&self) -> (usize, u8) {
        let (path, rank) = match self {
            Rule::Read(path) => (path, 0),
            Rule::Write(path) => (path, 1),
            Rule::Hide(path) => (path, 2),
        };
        (path.components().count(), rank)
    }
}

fn wrapped(cmd: Command, reach: &Reach<'_>, temp: Option<&Path>) -> Result<Command, NotSupported> {
    let program = os::program(path_of(&cmd).as_deref())?;
    let all = |paths: &[PathBuf]| paths.iter().map(|p| resolved(p)).collect::<Vec<_>>();
    let (writable, readable, hidden) =
        (all(reach.writable), all(reach.readable), all(reach.hidden));
    let mut rules: Vec<Rule<'_>> = readable.iter().map(|p| Rule::Read(p)).collect();
    rules.extend(writable.iter().map(|p| Rule::Write(p)));
    rules.extend(temp.map(Rule::Write));
    rules.extend(hidden.iter().map(|p| Rule::Hide(p)));
    // The longest path last, so its rule wins: the last rule wins on
    // macOS, and the last mount covers the ones before it on Linux.
    rules.sort_by_key(Rule::key);
    let args = os::args(&rules, temp.is_some(), reach.network);
    Ok(wrap(&program, &args, &cmd))
}

/// Why this host has no sandbox for a command that runs with `path` as its
/// `PATH`, or `None` when it has one. [`sandboxed`] gives the same
/// [`NotSupported`] value. Each caller that needs the sandbox host asks
/// here: macOS needs [`SANDBOX_EXEC`], and Linux needs `bwrap` in an
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

/// `path` as the kernel sees it, with each link resolved. A path that does
/// not exist yet keeps its resolved nearest existing folder and the rest of
/// its names. Seatbelt compares the resolved path: on macOS `/tmp` is
/// `/private/tmp`.
fn resolved(path: &Path) -> PathBuf {
    if let Ok(path) = path.canonicalize() {
        return path;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if parent != path => resolved(parent).join(name),
        _ => path.to_path_buf(),
    }
}
