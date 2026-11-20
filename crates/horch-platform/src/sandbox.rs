//! The concern sandbox: the check sandbox (SR-4), the sandbox host of PL-2.
//!
//! [`sandboxed`] wraps a command so that it can read the whole disk but
//! write only in a list of paths and in its temp folder.
//! [`sandboxed_hiding`] wraps a command so that it can write only in a list
//! of paths, cannot read, write or connect to a Unix socket below a list of
//! hidden paths, can read again a list of paths in them, and can have no
//! network.
//!
//! Each call also takes [`Denials`], a required input: the operator acts
//! that the command cannot do, whatever else the call allows (SR-8). A
//! worker, a check and a gate cannot read the operator token, connect to
//! the command channel or write a request file. What each OS denies:
//!
//! - macOS: `/usr/bin/sandbox-exec` with a generated Seatbelt profile. It
//!   denies each file read, file write and Unix socket connect below a
//!   denied read or connect path, and each file write below a denied write
//!   path. Seatbelt refuses the act (`EPERM`), also for a path that does
//!   not exist yet.
//! - Linux: `bwrap` (bubblewrap), when it is on the `PATH` of the command.
//!   It puts an empty file (`/dev/null`) over a denied read or connect file
//!   and an empty tmpfs over a denied folder: a read of the token gives no
//!   bytes, and a connect to the socket is refused (`ECONNREFUSED`). It
//!   binds a denied write path read-only, so a write is refused (`EROFS`).
//!   A denied path that does not exist when the command starts gets no
//!   mount, so it is not denied.
//! - Windows: [`NotSupported`]. A check never runs there without its
//!   sandbox: the caller decides what to do (PL-3).
//!
//! The OS module gives the sandbox host program and its arguments. This file puts
//! the command after them, so each OS keeps the program, arguments,
//! environment and folder of the command in the same way.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::os::sandbox as os;
use crate::start::{self, Sealed, Start, StdCommand, TOKEN};
use crate::NotSupported;

/// Where macOS keeps `sandbox-exec`, the Seatbelt launcher of the sandbox.
/// Only macOS uses it.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// `cmd` in the check sandbox: it can write only in the paths of
/// `writable`, in its temp folder ([`std::env::temp_dir`]) and in
/// `/dev/null`, and it can read the rest of the disk. `denied` wins over
/// each of these.
///
/// The returned command is of the kind of `cmd` (a [`start::Command`] or a
/// `std` command). It keeps the program, the arguments, the environment
/// changes and the current folder of `cmd`. A command does not show the
/// rest of its settings, so set stdio, `env_clear` and the OS extensions
/// on the returned command. Give absolute paths in `writable`.
///
/// It gives [`NotSupported`] with a reason when this host has no sandbox:
/// on Windows, on Linux when `bwrap` is not on the `PATH` of `cmd`, and on
/// macOS when [`SANDBOX_EXEC`] is missing.
pub fn sandboxed<C: Start>(
    cmd: C,
    writable: &[PathBuf],
    denied: &Denials,
) -> Result<C, NotSupported> {
    sandboxed_on(cmd, writable, denied, Network::Open)
}

/// [`sandboxed`] with [`Network::Local`]: the command has no off-host
/// network (SR-3: no agent messages anyone outside the fleet). It gives
/// [`NotSupported`] on a host where the platform layer cannot hold this
/// rule, and never opens the network instead.
pub fn sandboxed_local<C: Start>(
    cmd: C,
    writable: &[PathBuf],
    denied: &Denials,
) -> Result<C, NotSupported> {
    sandboxed_on(cmd, writable, denied, Network::Local)
}

fn sandboxed_on<C: Start>(
    cmd: C,
    writable: &[PathBuf],
    denied: &Denials,
    network: Network,
) -> Result<C, NotSupported> {
    let temp = resolved(&std::env::temp_dir());
    let reach = Reach {
        writable,
        readable: &[],
        hidden: &[],
        denied,
        network,
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
    /// The command can connect to this host only: loopback and Unix
    /// sockets. Each connection to another host is refused. On macOS a
    /// Seatbelt rule denies each outbound IP connection but `localhost`.
    /// On Linux the command gets a new network namespace, where `bwrap`
    /// brings up the loopback interface. Windows has no sandbox, so it
    /// gives [`NotSupported`].
    Local,
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
    /// The acts that the command cannot do, whatever the other fields
    /// allow.
    pub denied: &'a Denials,
    pub network: Network,
}

/// The acts that a sandboxed command cannot do, whatever the rest of the
/// call allows: a denial wins over each writable, readable or hidden path,
/// also over a longer one. Give absolute paths. The module doc names what
/// each OS denies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Denials {
    /// The command cannot read these files, or below these folders. It
    /// cannot write them or connect to a Unix socket below them either.
    pub read: Vec<PathBuf>,
    /// The command cannot connect to a Unix socket at or below these paths.
    /// It cannot read or write them either.
    pub connect: Vec<PathBuf>,
    /// The command cannot write at or below these paths. It can read them
    /// when the rest of the call lets it.
    pub write: Vec<PathBuf>,
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
/// A program makes the leading folders of a path with a `stat` of each
/// parent folder. So the command can read the metadata of each folder
/// from a hidden path down to the parent of a readable or writable path in
/// it. It cannot list such a folder or read a file in it.
///
/// - macOS: Seatbelt checks each file read and write and each Unix socket
///   connection against the rules, whether the path exists or not. Each
///   such parent folder allows a metadata read of that folder only, so
///   the command sees its real mode, owner, size, link count and times.
/// - Linux: the rules are mounts, from the shortest path to the longest.
///   A new empty tmpfs covers each hidden folder, and `/dev/null` covers
///   each hidden file, so a socket file in a hidden folder is gone. A path
///   that does not exist when the command starts gets no mount: a hidden
///   one is not covered, a readable one in a hidden folder is not there,
///   and a writable one is not writable. So give writable paths that
///   exist. An abstract Unix socket has no path: it is not denied (but see
///   [`Network::Denied`]). A bound path is a mount point: the command
///   cannot rename or remove it, only what is in it. bwrap makes each
///   parent folder of a bound path in the tmpfs of a hidden folder. Such a
///   folder holds only the next part of the bound path, and its metadata
///   is of the new folder, not of the real one.
///
/// The rest is as in [`sandboxed`]: the command and its settings, and
/// [`NotSupported`] on a host with no sandbox.
pub fn sandboxed_hiding<C: Start>(cmd: C, reach: &Reach<'_>) -> Result<C, NotSupported> {
    wrapped(cmd, reach, None)
}

/// 1 rule of a sandbox on 1 resolved path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule<'a> {
    Read(&'a Path),
    Write(&'a Path),
    Hide(&'a Path),
}

impl<'a> Rule<'a> {
    pub(crate) fn path(&self) -> &'a Path {
        match *self {
            Rule::Read(path) | Rule::Write(path) | Rule::Hide(path) => path,
        }
    }

    fn key(&self) -> (usize, u8) {
        let (path, rank) = match self {
            Rule::Read(path) => (path, 0),
            Rule::Write(path) => (path, 1),
            Rule::Hide(path) => (path, 2),
        };
        (path.components().count(), rank)
    }
}

fn wrapped<C: Start>(cmd: C, reach: &Reach<'_>, temp: Option<&Path>) -> Result<C, NotSupported> {
    let cmd = cmd.std(TOKEN);
    let program = os::program(path_of(cmd).as_deref())?;
    let all = |paths: &[PathBuf]| paths.iter().map(|p| resolved(p)).collect::<Vec<_>>();
    let (writable, readable, hidden) =
        (all(reach.writable), all(reach.readable), all(reach.hidden));
    let mut rules: Vec<Rule<'_>> = readable.iter().map(|p| Rule::Read(p)).collect();
    rules.extend(writable.iter().map(|p| Rule::Write(p)));
    rules.extend(temp.map(Rule::Write));
    rules.extend(hidden.iter().map(|p| Rule::Hide(p)));
    let mut sealed = all(&reach.denied.read);
    sealed.extend(all(&reach.denied.connect));
    let read_only = all(&reach.denied.write);
    let mut rules = denying(rules, &sealed, &read_only);
    // The longest path last, so its rule wins: the last rule wins on
    // macOS, and the last mount covers the ones before it on Linux.
    rules.sort_by_key(Rule::key);
    let args = os::args(&rules, temp.is_some(), reach.network);
    Ok(C::from_std(wrap(&program, &args, cmd), TOKEN))
}

/// `rules` with the denials put in, so that no other rule undoes them.
/// Each path of `read_only` gets the rule that denies a write: each
/// writable rule at or below it becomes readable, and the path itself gets
/// a readable rule when the rule that reaches it allows a write. Each path
/// of `sealed` gets a hidden rule, and each other rule at or below it goes.
fn denying<'a>(
    mut rules: Vec<Rule<'a>>,
    sealed: &'a [PathBuf],
    read_only: &'a [PathBuf],
) -> Vec<Rule<'a>> {
    for path in read_only {
        let reaching = rules
            .iter()
            .filter(|rule| path.starts_with(rule.path()))
            .max_by_key(|rule| rule.key())
            .copied();
        for rule in &mut rules {
            if let Rule::Write(below) = *rule {
                if below.starts_with(path) {
                    *rule = Rule::Read(below);
                }
            }
        }
        if let Some(Rule::Write(_)) = reaching {
            rules.push(Rule::Read(path));
        }
    }
    for path in sealed {
        rules.retain(|rule| !rule.path().starts_with(path));
        rules.push(Rule::Hide(path));
    }
    rules
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
fn path_of(cmd: &StdCommand) -> Option<OsString> {
    match cmd.get_envs().find(|(key, _)| *key == "PATH") {
        Some((_, value)) => value.map(OsString::from),
        None => std::env::var_os("PATH"),
    }
}

/// A command that runs `program` with `args`, then the program of `cmd`.
fn wrap(program: &Path, args: &[OsString], cmd: &StdCommand) -> StdCommand {
    let mut sandboxed = start::command(program).into_std(TOKEN);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::process::Command;
    use std::time::Duration;

    /// The address of the child test: an address that no host answers
    /// (RFC 5737, TEST-NET-1). A connect to it never reaches a webhook.
    const OFF_HOST: &str = "192.0.2.1:9";
    const TARGET: &str = "SANDBOX_CHILD_TARGET";

    #[test]
    #[ignore = "run by the sandbox tests, inside a sandbox"]
    fn connect_child() {
        let target: SocketAddr = std::env::var(TARGET).expect(TARGET).parse().unwrap();
        match TcpStream::connect_timeout(&target, Duration::from_secs(3)) {
            Ok(_) => println!("RESULT connected"),
            Err(e) => println!("RESULT refused {:?} {:?}", e.kind(), e.raw_os_error()),
        }
    }

    fn connect_in_local_sandbox(target: SocketAddr) -> Option<String> {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "sandbox::tests::connect_child", "--ignored"])
            .args(["--nocapture", "--test-threads=1"])
            .env(TARGET, target.to_string());
        let mut cmd = match sandboxed_local(cmd, &[], &Denials::default()) {
            Ok(cmd) => cmd,
            Err(_) => {
                assert!(
                    host_problem(None).is_some(),
                    "no sandbox on a host with one"
                );
                return None;
            }
        };
        let out = cmd.output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "{text}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let line = text.split("RESULT ").nth(1).and_then(|r| r.lines().next());
        Some(
            line.unwrap_or_else(|| panic!("the child printed no result: {text}"))
                .to_string(),
        )
    }

    #[test]
    fn local_network_allows_loopback_and_refuses_an_off_host_address() {
        let webhook = TcpListener::bind("127.0.0.1:0").unwrap();
        let Some(line) = connect_in_local_sandbox(webhook.local_addr().unwrap()) else {
            return;
        };
        assert_eq!(line, "connected");
        assert!(webhook.accept().is_ok(), "the webhook got no connection");

        let line = connect_in_local_sandbox(OFF_HOST.parse().unwrap()).unwrap();
        assert!(line.starts_with("refused"), "{line}");
        assert!(
            !line.contains("TimedOut"),
            "a timeout is no refusal: {line}"
        );
        #[cfg(target_os = "macos")]
        assert!(
            line.contains("PermissionDenied"),
            "Seatbelt gives EPERM: {line}"
        );
    }
}
