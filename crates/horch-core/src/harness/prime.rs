//! Prime Agent-specific glue: containing its daemon, and finding the session it
//! wrote. Written for the Rust build of Prime Agent (0.10.0).
//!
//! Prime Agent supervises its own agents in a background daemon that outlives
//! the CLI. herdr already supervises - a pane IS the agent's lifetime - so left
//! alone, `horch done` closing a worker's pane would leave that worker's Prime
//! daemon running, and a fleet would accumulate one per spawn.
//!
//! Each launch gets its own launch dir, `<state_root>/prime/<id>` (`<id>`: 12
//! hex digits), and its own daemon socket in it,
//! `<launch>/prime-agent-<uid>/d.sock`, passed as `--daemon-socket`. The
//! socket's dir has that name because `prime-agent shutdown` and `prime-agent
//! status` ignore `--daemon-socket`: they act on every daemon whose socket is
//! in `$TMPDIR/prime-agent-<uid>/`. With `TMPDIR=<launch>/` they act on this
//! launch's daemon and nothing else, so [`Daemon::finish`] stops exactly this
//! pane's daemon and leaves the operator's own alone. A bare `prime-agent
//! shutdown` would stop every agent on the machine, including other panes in
//! the same fleet.
//!
//! Prime reports the pane's agent state to herdr itself: the Rust daemon has a
//! built-in herdr reporter (source `herdr:pi`) that reads the pane's `HERDR_*`
//! env and cannot be turned off. horch only releases the pane's agent when the
//! pane finishes, as a clean-up after a crash.
//!
//! Sessions are the other half. Prime has no `--session-id` (pi does), so horch
//! cannot name the session before launch. It can own the directory: with
//! `--session-dir` pointing somewhere only this launch writes, the session file
//! that appears there IS this worker's, with no timestamps or project paths to
//! disambiguate.
//!
//! The third part is the agent dir (design `ai_docs/plans/wave2/w1/design.md`
//! §6.9). Prime reads its settings and `models.json` from
//! `$PRIME_AGENT_CODING_AGENT_DIR`, so each launch gets its own: every entry
//! of the operator's dir is a link (`auth.json` included: Prime writes a
//! refreshed token through the link onto the operator's file), and only
//! `settings.json` and `models.json` are generated. The fleet window goes in a
//! `models.json` that holds the override and nothing else, and only when the
//! operator's own files set no window for the model's provider.
//!
//! Launch dirs of Prime 0.9.4 runs have the socket at `<launch>/d.sock`. The
//! sweep and [`finish_pane`] handle that layout too; the Rust `prime-agent`
//! cannot name or stop such a daemon, so horch leaves a live one running.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result};

use serde_json::{json, Value};

use super::claude::{default_leaves, insert_at_path};
use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, PrepareRequest, Prepared, WindowInputs};
use crate::compaction::window::{OperatorWindow, WindowDecision};
use crate::roster::{HarnessDefault, Teammate};
use crate::runtime::RuntimeContext;
use crate::skills::Bundle;

/// The daemon socket's file name, in the launch's scan dir (0.9.4: in the
/// launch dir itself).
const SOCKET: &str = "d.sock";
/// The longest socket path `Daemon::install` accepts: a Unix socket path holds
/// at most 104 bytes on macOS, the last one a NUL.
pub const MAX_SOCKET_BYTES: usize = 103;

/// The dir name `prime-agent status` and `shutdown` scan under `$TMPDIR`:
/// `prime-agent-<uid>`, with the real uid of this process.
pub fn scan_dir_name() -> String {
    format!("prime-agent-{}", real_uid())
}

#[cfg(unix)]
fn real_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn real_uid() -> u32 {
    0
}

/// The daemon socket of the launch dir `launch`, if it has one: the Rust
/// layout `<launch>/prime-agent-<uid>/d.sock`, else the 0.9.4 layout
/// `<launch>/d.sock`.
fn launch_socket(launch: &Path) -> Option<PathBuf> {
    [
        launch.join(scan_dir_name()).join(SOCKET),
        launch.join(SOCKET),
    ]
    .into_iter()
    .find(|s| std::fs::symlink_metadata(s).is_ok())
}

/// One Prime Agent launch's dir, private daemon socket and session directory.
#[derive(Debug, Clone)]
pub struct Daemon {
    launch: PathBuf,
    socket: PathBuf,
    sessions: PathBuf,
    /// The `prime-agent` program that answers `shutdown` and `status` for
    /// [`Daemon::finish`].
    bin: PathBuf,
    /// When this launch reserved its socket. Its daemon starts later; a
    /// process that started earlier is not it.
    installed: SystemTime,
}

impl Daemon {
    /// Reserve a launch dir, a socket and a session directory. `bin` is the
    /// `prime-agent` program the launch runs. Fails when the socket path is
    /// longer than [`MAX_SOCKET_BYTES`].
    pub fn install(state_root: &Path, bin: &Path) -> Result<Daemon> {
        // No role slug: the socket path must stay short.
        let id: String = crate::mint_uuid()
            .chars()
            .filter(char::is_ascii_hexdigit)
            .take(12)
            .collect();
        let launch = state_root.join("prime").join(id);
        let scan = launch.join(scan_dir_name());
        let socket = scan.join(SOCKET);
        let len = socket.as_os_str().len();
        if len > MAX_SOCKET_BYTES {
            anyhow::bail!(
                "the Prime daemon socket path {} is {len} bytes, over the limit of \
                 {MAX_SOCKET_BYTES} bytes; set HORCH_STATE_DIR to a shorter dir",
                socket.display()
            );
        }
        let sessions = launch.join("sessions");
        // Owner only: the session files hold the whole conversation.
        crate::fsx::ensure_private_dir(&launch)?;
        crate::fsx::ensure_private_dir(&scan)?;
        crate::fsx::ensure_private_dir(&sessions)?;
        // Who runs this launch, so a later launch's sweep leaves its agent
        // dir alone while it runs.
        let pid = std::process::id();
        let started = crate::procid::start_time(pid)
            .map(|t| t.to_string())
            .unwrap_or_default();
        crate::fsx::write_atomic(
            &launch.join(LAUNCHER),
            format!("{pid} {started}\n").as_bytes(),
            crate::fsx::PRIVATE_FILE,
        )?;
        Ok(Daemon {
            launch,
            socket,
            sessions,
            bin: bin.to_path_buf(),
            installed: SystemTime::now(),
        })
    }

    /// The daemon of an earlier launch in `launch`, with `socket` from
    /// [`launch_socket`] and its launcher file's time as the install time.
    /// None without a launcher file: that launch's start is unknown.
    fn of_launch(launch: &Path, socket: PathBuf, bin: &Path) -> Option<Daemon> {
        let installed = std::fs::metadata(launch.join(LAUNCHER))
            .and_then(|m| m.modified())
            .ok()?;
        Some(Daemon {
            launch: launch.to_path_buf(),
            socket,
            sessions: launch.join("sessions"),
            bin: bin.to_path_buf(),
            installed,
        })
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions
    }

    /// This launch's dir: it holds the socket's scan dir, the sessions and
    /// the agent dir.
    pub fn launch_dir(&self) -> &Path {
        &self.launch
    }

    /// This launch's agent dir.
    fn agent_dir(&self) -> PathBuf {
        self.launch.join(AGENT_DIR)
    }

    /// The `TMPDIR` that scopes `prime-agent shutdown` and `status` to this
    /// launch's scan dir.
    fn tmpdir(&self) -> std::ffi::OsString {
        let mut dir = self.launch.clone().into_os_string();
        dir.push("/");
        dir
    }

    /// Stop this launch's daemon, and only this one.
    ///
    /// No process listens on the socket: there is no daemon (Prime never
    /// started one, or it stopped and left the socket file). Else first
    /// `TMPDIR=<launch>/ prime-agent shutdown --force --json`: it stops the
    /// supervisor and its workers, and the stop counts only when its
    /// `stopped` list names this socket. The agent dir, which holds the
    /// worker records, is removed only after a stop.
    ///
    /// When that stop fails: `TMPDIR=<launch>/ prime-agent status --json`
    /// names the pid; a pid can be given to another program once its
    /// process ends. So `status` is asked twice, and SIGTERM goes only to a
    /// pid that both answers name with the same start time, and that
    /// started after [`Daemon::install`]: horch never stops a daemon it did
    /// not start, such as the operator's own. A SIGTERM to the supervisor
    /// also ends its workers within about 4 s. Otherwise the daemon is left
    /// running, and the reason is logged. A daemon left running keeps its
    /// socket and its agent dir.
    ///
    /// Residual risk (U-63): Prime reports no start time of its own.
    /// Prime takes the pid from `lsof`: a process that holds the socket. So a
    /// wrong SIGTERM needs a process that started after this launch, got the
    /// dead daemon's pid and holds this launch's private socket.
    pub fn finish(&self) {
        if let Some(line) = self.stop() {
            // Not `eprintln!`: after a hangup stderr is gone, and a failed
            // write must not end the cleanup.
            use std::io::Write as _;
            let _ = writeln!(std::io::stderr(), "{line}");
        }
    }

    /// [`Daemon::finish`] without the log: the line that says why the
    /// daemon was left running, or None when it is gone and the files are
    /// removed.
    pub(crate) fn stop(&self) -> Option<String> {
        let problem = listening(&self.socket)
            .then(|| self.shutdown_problem())
            .flatten();
        if problem.is_none() {
            self.remove_launch_files();
        }
        problem
    }

    /// Why the daemon was left running after a stop, or None when it is
    /// gone.
    fn shutdown_problem(&self) -> Option<String> {
        match self.shutdown() {
            Stop::Stopped => None,
            // The stop process goes on alone; a later sweep cleans up.
            Stop::TimedOut => Some(format!(
                "horch: the Prime stop for {} still runs after {} s; going on",
                self.socket.display(),
                STOP_WAIT.as_secs()
            )),
            Stop::Failed => self.terminate_supervisor(),
        }
    }

    fn remove_launch_files(&self) {
        let _ = std::fs::remove_file(&self.socket);
        // The scan dir of the Rust layout, when it is empty now.
        if let Some(scan) = self.socket.parent().filter(|s| *s != self.launch) {
            let _ = std::fs::remove_dir(scan);
        }
        // The agent dir links the operator's live `auth.json`; it is not
        // needed after the pane. `remove_dir_all` does not follow links. The
        // sessions stay for a resume.
        let _ = std::fs::remove_dir_all(self.agent_dir());
    }

    /// `TMPDIR=<launch>/ prime-agent shutdown --force --json`, at most
    /// [`STOP_WAIT`]. Without `--force` Prime refuses ("confirmation
    /// required").
    ///
    /// The stop process runs in its own session (`spawn_detached`): `horch
    /// done` in a Prime pane runs under the daemon's worker, and the stop
    /// ends that worker; the stop must not end with it (live check X2:
    /// prime-6). horch waits for it, so it is not a background job.
    fn shutdown(&self) -> Stop {
        let mut cmd = Command::new(&self.bin);
        cmd.args(["shutdown", "--force", "--json"])
            .env("TMPDIR", self.tmpdir())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let Ok(mut child) = crate::runtime::process::spawn_detached(&mut cmd) else {
            return Stop::Failed;
        };
        match wait_for_stop(&mut child, STOP_WAIT) {
            Waited::Exited(true) => {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    use std::io::Read as _;
                    let _ = stdout.read_to_string(&mut out);
                }
                if stopped_names(&out, &self.socket) {
                    Stop::Stopped
                } else {
                    Stop::Failed
                }
            }
            Waited::Exited(false) => Stop::Failed,
            Waited::TimedOut => Stop::TimedOut,
        }
    }

    /// SIGTERM to the supervisor `status` names, under the rules above.
    /// The line that says why it was left running, or None after the
    /// SIGTERM.
    fn terminate_supervisor(&self) -> Option<String> {
        let Some(pid) = self.daemon_pid() else {
            return Some(format!(
                "horch: left Prime daemon {} running: status names no pid",
                self.socket.display()
            ));
        };
        let first = pid_start(pid);
        let again = self.daemon_pid().map(|p| (p, pid_start(p)));
        let started_at = u32::try_from(pid).ok().and_then(crate::procid::started_at);
        match may_stop((pid, first), again, started_at, self.installed) {
            Ok(()) => {
                terminate(pid, first);
                None
            }
            Err(why) => Some(format!("horch: left Prime daemon pid {pid} running: {why}")),
        }
    }

    /// The pid of the Prime daemon listening on this launch's socket, from
    /// `TMPDIR=<launch>/ prime-agent status --json`.
    fn daemon_pid(&self) -> Option<i32> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(["status", "--json"])
            .env("TMPDIR", self.tmpdir())
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::runtime::process::scrub_child_env(&mut cmd);
        let output = cmd.output().ok()?;
        if !output.status.success() {
            return None;
        }
        pid_for_socket(&String::from_utf8_lossy(&output.stdout), &self.socket)
    }
}

/// Whether a process listens on the Unix socket at `socket`. No file, a
/// refused connection or a file that is not a socket: no. Any other error
/// counts as yes, so the stop rules decide.
#[cfg(unix)]
fn listening(socket: &Path) -> bool {
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(_) => true,
        Err(e) => {
            !(matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) || e.raw_os_error() == Some(libc::ENOTSOCK))
        }
    }
}

#[cfg(not(unix))]
fn listening(socket: &Path) -> bool {
    std::fs::symlink_metadata(socket).is_ok()
}

/// How the wait for a stop process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Waited {
    /// It exited; whether with success.
    Exited(bool),
    /// Still running at the limit: it goes on alone.
    TimedOut,
}

/// Wait for the stop process at most `limit`. On timeout the process goes
/// on alone.
fn wait_for_stop(child: &mut std::process::Child, limit: std::time::Duration) -> Waited {
    let deadline = std::time::Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => break Waited::Exited(status.success()),
            Err(_) => break Waited::Exited(false),
            Ok(None) if std::time::Instant::now() >= deadline => break Waited::TimedOut,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
}

/// How long horch waits for `prime-agent shutdown`: it stopped a daemon and
/// its worker in about 2 s on 0.10.0; a loaded machine is slower.
pub const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// What [`Daemon::shutdown`] gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// The `stopped` list names the socket.
    Stopped,
    /// Prime's stop could not run or did not stop it: the caller falls back.
    Failed,
    /// Still running at [`STOP_WAIT`]: it goes on alone.
    TimedOut,
}

/// Whether the `prime-agent shutdown --json` output `json`
/// (`{"stopped": [{"socketPath": .., "action": ..}], "failed": [..]}`)
/// names `socket` as stopped.
pub(crate) fn stopped_names(json: &str, socket: &Path) -> bool {
    serde_json::from_str::<Value>(json).is_ok_and(|doc| {
        doc.get("stopped")
            .and_then(Value::as_array)
            .is_some_and(|stopped| {
                stopped.iter().any(|r| {
                    r.get("socketPath")
                        .and_then(Value::as_str)
                        .is_some_and(|p| same_socket(Path::new(p), socket))
                })
            })
    })
}

/// Whether Prime's path `reported` names `socket`: equal, or equal once both
/// dirs are resolved (a state root under a linked dir such as macOS `/tmp`).
fn same_socket(reported: &Path, socket: &Path) -> bool {
    if reported == socket {
        return true;
    }
    let dir = |p: &Path| p.parent().and_then(|d| std::fs::canonicalize(d).ok());
    reported.file_name() == socket.file_name()
        && dir(reported).is_some()
        && dir(reported) == dir(socket)
}

/// How far a start time on the wall clock can be off: Linux counts it from a
/// boot time in whole seconds.
const START_SLACK: std::time::Duration = std::time::Duration::from_secs(1);

fn pid_start(pid: i32) -> Option<u64> {
    u32::try_from(pid).ok().and_then(crate::procid::start_time)
}

/// Whether [`Daemon::finish`] may signal the daemon that `status` named:
/// `first` and `again` are the 2 answers, each a pid and its start time.
/// Split out so the rules are testable without Prime installed.
pub(crate) fn may_stop(
    first: (i32, Option<u64>),
    again: Option<(i32, Option<u64>)>,
    started_at: Option<SystemTime>,
    installed: SystemTime,
) -> std::result::Result<(), &'static str> {
    let again = again.ok_or("a second status call does not name it")?;
    require(again.0 == first.0, "a second status call names another pid")?;
    if cfg!(windows) {
        // No start times here: the pid is all there is (see `procid`).
        Ok(())
    } else {
        require(
            first.1.is_some() && again.1 == first.1,
            "its start time is unknown or changed between 2 status calls",
        )?;
        require(
            started_at.is_some_and(|t| t + START_SLACK >= installed),
            "it started before this launch, so this launch did not start it",
        )
    }
}

fn require(ok: bool, why: &'static str) -> std::result::Result<(), &'static str> {
    if ok {
        Ok(())
    } else {
        Err(why)
    }
}

/// The pid that the `prime-agent status --json` output `json` gives for
/// `socket`. Split out from [`Daemon::daemon_pid`] so the matching is
/// testable without Prime installed. A record with no pid (`orphan-file`: a
/// socket file with no daemon) gives None.
pub(crate) fn pid_for_socket(json: &str, socket: &Path) -> Option<i32> {
    let records: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    records
        .iter()
        .find_map(|r| {
            let path = r.get("socketPath")?.as_str()?;
            same_socket(Path::new(path), socket)
                .then(|| r.get("pid")?.as_i64())
                .flatten()
        })
        // A pid that names no single process (0, or one that turns negative
        // as a `pid_t`) names no daemon.
        .and_then(|pid| u32::try_from(pid).ok().and_then(crate::procid::os_pid))
}

/// SIGTERM to `pid` while it still has start time `started` (see
/// [`crate::procid::signal_same`]).
#[cfg(unix)]
fn terminate(pid: i32, started: Option<u64>) {
    // SIGTERM, not SIGKILL: the daemon flushes its sessions on the way out, and
    // a session file half-written is a resume that fails later in a pane nobody
    // is watching.
    if let (Ok(pid), Some(started)) = (u32::try_from(pid), started) {
        crate::procid::signal_same(pid, started, libc::SIGTERM);
    }
}

#[cfg(not(unix))]
fn terminate(pid: i32, _started: Option<u64>) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status();
}

/// The newest session file Prime wrote into a directory this launch owns.
///
/// No filtering by project or time: nothing else writes here, so whatever
/// appears is this worker's session.
pub fn find_session(sessions_dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(sessions_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_session = path.extension().and_then(|e| e.to_str()) == Some("jsonl");
            let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
            is_session.then_some((modified, path))
        })
        // The first of equally new files wins.
        .min_by_key(|(modified, _)| std::cmp::Reverse(*modified))
        .map(|(_, path)| path)
}

/// The variable Prime reads its agent dir from (0.9.4 `dist/config.js`).
pub const AGENT_DIR_ENV: &str = "PRIME_AGENT_CODING_AGENT_DIR";
/// The 2 agent-dir files a launch generates; every other entry is a link.
const SETTINGS: &str = "settings.json";
const MODELS: &str = "models.json";
/// Always linked, also when the source has none: a login in the pane then
/// lands in the operator's file, not in a dir that is removed.
const AUTH: &str = "auth.json";
/// Prime's global harness state dir (0.9.4 `core/refinement/refinement.js`).
const HARNESS_STATE: &str = "harness";
/// The launch's agent dir and the file that names its launcher.
const AGENT_DIR: &str = "agent";
const LAUNCHER: &str = "launcher";
/// The herdr pane a launch ran in, for [`finish_pane`].
const PANE: &str = "pane";

/// The agent dir a launch links from: the teammate's own
/// `PRIME_AGENT_CODING_AGENT_DIR`, else the inherited one, else
/// `~/.prime/agent`. A leading `~/` is expanded against home, as Prime does.
/// An inherited value inside `<state_root>/prime/` is another launch's agent
/// dir (a pane started from a Prime pane) and is ignored. Absolute, so a
/// link to an entry resolves from the launch's agent dir too.
pub(crate) fn source_agent_dir(ctx: &RuntimeContext, teammate: &Teammate) -> PathBuf {
    let home = &ctx.paths.home;
    let expand = |p: &str| crate::roster::expand_home(p, Some(home));
    let own = teammate
        .env
        .get(AGENT_DIR_ENV)
        .filter(|v| !v.is_empty())
        .map(|v| expand(v));
    let launches = absolute(&ctx.paths.state_root.join("prime"));
    let inherited = ctx
        .inherited
        .prime_agent_dir
        .as_deref()
        .map(|p| p.to_str().map(expand).unwrap_or_else(|| p.to_path_buf()))
        .filter(|p| !absolute(p).starts_with(&launches));
    absolute(
        &own.or(inherited)
            .unwrap_or_else(|| home.join(".prime").join("agent")),
    )
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Clean up earlier launches under `prime_root` whose launcher is gone: for
/// panes killed before [`Daemon::finish`] ran. A launch with a daemon
/// socket (either layout, see [`launch_socket`]) gets [`Daemon::finish`]
/// with its launcher file's time as the install time, so only a daemon that
/// launch started is stopped; a launch with no launcher file keeps its
/// daemon. Without a socket, its agent dir is removed. `bin` answers
/// `shutdown` and `status`. Best effort: an error leaves the entry.
fn sweep_agent_dirs(prime_root: &Path, bin: &Path) {
    let Ok(entries) = std::fs::read_dir(prime_root) else {
        return;
    };
    for entry in entries.flatten() {
        let launch = entry.path();
        if launcher_alive(&launch) {
            continue;
        }
        match launch_socket(&launch) {
            Some(socket) => {
                if let Some(daemon) = Daemon::of_launch(&launch, socket, bin) {
                    daemon.finish();
                }
            }
            None => {
                let _ = std::fs::remove_dir_all(launch.join(AGENT_DIR));
            }
        }
    }
}

/// Stop the daemon of every launch that ran in herdr pane `pane` and still
/// has a socket (either layout), through [`Daemon::finish`] with the
/// launcher file's time as the install time. `horch done` runs it after it
/// closes the pane: herdr sends SIGHUP, then SIGTERM, then SIGKILL to a
/// closed pane's processes within about 1 s, so the launcher's own finish
/// often does not run. `horch done` itself runs under Prime's daemon, in
/// another session, so the close does not end it; a stop before the close
/// could. `bin` answers `shutdown` and `status`. Does nothing for a pane
/// that ran no Prime launch.
pub fn finish_pane(state_root: &Path, bin: &Path, pane: &str) {
    let Ok(entries) = std::fs::read_dir(state_root.join("prime")) else {
        return;
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|launch| ran_in_pane(launch, pane))
        .filter_map(|launch| {
            let socket = launch_socket(&launch)?;
            Daemon::of_launch(&launch, socket, bin)
        })
        .for_each(|daemon| daemon.finish());
}

fn ran_in_pane(launch: &Path, pane: &str) -> bool {
    std::fs::read_to_string(launch.join(PANE)).is_ok_and(|p| p.trim() == pane)
}

/// Keep this launcher alive when its pane closes. Closing a pane hangs up
/// its terminal: SIGHUP goes to the pane's foreground process group, which
/// is horch and the Prime CLI. Prime's daemon runs in its own group, so it
/// does not get it. Before this, the default action ended horch before
/// [`Prepared::finish`], and the daemon ran on (a C5 daemon ran 6 h 44 min
/// after `horch done`). A handler, not SIG_IGN: an exec resets it, so the
/// Prime CLI still ends on the hangup, and then horch stops the daemon.
#[cfg(unix)]
fn outlive_hangup() {
    extern "C" fn on_hangup(_: libc::c_int) {}
    // SAFETY: the handler does nothing, so it is async-signal-safe.
    unsafe {
        let handler = on_hangup as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGHUP, handler);
    }
}

#[cfg(not(unix))]
fn outlive_hangup() {}

/// The `--source` and `--agent` of the pane reports of Prime's built-in
/// herdr reporter (`pa-daemon/src/herdr.rs`), for the release on finish. A
/// release with another source or agent label does nothing. The label is
/// the one the live check saw (`docs/live-checks/harnesses.md`).
pub const STATUS_SOURCE: &str = "herdr:pi";
pub const STATUS_AGENT: &str = "prime-agent";

/// A `--seq` for the release on finish: the time in ms × 1000 + 999.
fn release_seq() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64 * 1000 + 999)
        .unwrap_or_default()
}

/// Whether the horch process that wrote `<launch>/launcher` still runs.
fn launcher_alive(launch: &Path) -> bool {
    read_launcher(launch).is_some_and(|(pid, started)| crate::procid::alive(pid, started))
}

/// The pid and start time (when recorded) in `<launch>/launcher`.
fn read_launcher(launch: &Path) -> Option<(u32, Option<u64>)> {
    let text = std::fs::read_to_string(launch.join(LAUNCHER)).ok()?;
    let mut fields = text.split_whitespace();
    let pid = fields.next()?.parse().ok()?;
    let started = fields.next().and_then(|t| t.parse().ok());
    Some((pid, started))
}

/// `<workdir>/.prime/agent/settings.json`: Prime's project settings.
fn project_settings(workdir: &Path) -> PathBuf {
    workdir.join(".prime").join("agent").join(SETTINGS)
}

/// The window the operator's Prime files set for `model` (design §6.4,
/// review finding 10): `models` is the agent dir's `models.json`;
/// `settings` are the agent dir's and the project's `settings.json`. A
/// `models.json` that defines the model's provider at all is the
/// operator's, with or without a window for this model; so is one horch
/// cannot parse or read. A `compaction` setting moves Prime's trigger by an
/// amount horch does not model.
fn prime_operator_window(
    models: &Path,
    settings: &[PathBuf],
    model: &str,
) -> Option<OperatorWindow> {
    models_override(models, model)
        .or_else(|| settings.iter().find_map(|path| settings_override(path)))
}

/// A file horch refuses to read (a FIFO, over 1 MiB) is the operator's,
/// trigger unknown.
fn read_operator_file(path: &Path) -> std::result::Result<Option<String>, OperatorWindow> {
    crate::fsx::read_regular_bounded(path).map_err(|why| OperatorWindow {
        tokens: None,
        detail: format!("{} ({})", path.display(), why.reason()),
    })
}

fn models_override(models: &Path, model: &str) -> Option<OperatorWindow> {
    match read_operator_file(models) {
        Err(window) => Some(window),
        Ok(Some(text)) => models_window(&models.to_string_lossy(), &text, model),
        Ok(None) => None,
    }
}

fn settings_override(path: &Path) -> Option<OperatorWindow> {
    match read_operator_file(path) {
        Err(window) => Some(window),
        Ok(Some(text)) if compaction_set(&text) => Some(OperatorWindow {
            tokens: None,
            detail: format!("{} (compaction set; trigger unknown)", path.display()),
        }),
        Ok(_) => None,
    }
}

/// What a `models.json` says about `<p>/<m>`: None when it does not define
/// provider `<p>`. The tokens are the override for `<m>`, else the
/// `contextWindow` of the custom model `<m>`.
fn models_window(path: &str, text: &str, model: &str) -> Option<OperatorWindow> {
    let (provider, id) = model.split_once('/')?;
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return Some(OperatorWindow {
            tokens: None,
            detail: format!("{path} (not valid JSON)"),
        });
    };
    let defined = doc.get("providers")?.get(provider)?;
    let custom = || {
        defined
            .get("models")?
            .as_array()?
            .iter()
            .find(|m| m.get("id").and_then(Value::as_str) == Some(id))?
            .get("contextWindow")?
            .as_u64()
    };
    let tokens = defined
        .get("modelOverrides")
        .and_then(|o| o.get(id))
        .and_then(|o| o.get("contextWindow"))
        .and_then(Value::as_u64)
        .or_else(custom);
    Some(OperatorWindow {
        tokens,
        detail: format!("{path} (provider {provider} defined)"),
    })
}

/// Whether a `settings.json` sets `compaction.reserveTokens` or
/// `compaction.enabled`.
fn compaction_set(text: &str) -> bool {
    serde_json::from_str::<Value>(text).is_ok_and(|doc| {
        doc.get("compaction")
            .is_some_and(|c| c.get("reserveTokens").is_some() || c.get("enabled").is_some())
    })
}

/// The fleet `models.json` for an applied decision: the override for the
/// launch's model and nothing else. None: the operator's file is linked.
fn fleet_models(decision: Option<&WindowDecision>, model: &str) -> Option<Value> {
    let decision = decision.filter(|d| d.applied)?;
    let tokens = decision.tokens?;
    let (provider, id) = model.split_once('/')?;
    Some(json!({"providers": {provider: {"modelOverrides": {id: {"contextWindow": tokens}}}}}))
}

/// The agent dir's `settings.json`: the operator's object, without the
/// legacy `apiKeys` (Prime 0.9.4 reads it only in a migration), plus each
/// `agent_settings` key path of the teammate's Prime harness defaults that
/// neither the operator's file nor the project's sets (a `force` entry sets
/// it over the operator's file; a later entry wins). None: no key to add,
/// or the operator's file is not a JSON object; the operator's file is then
/// linked as it is, so no copy of it exists.
fn fleet_settings(
    operator: Option<&str>,
    project: Option<&str>,
    teammate: &Teammate,
) -> Option<serde_json::Map<String, Value>> {
    let mut settings = operator.map_or(Some(serde_json::Map::new()), parse_object)?;
    let project: Option<Value> = project.and_then(|t| serde_json::from_str(t).ok());
    let mut inserted = false;
    for (path, value, forced) in prime_leaves(teammate) {
        let operator_sets = sets_path(&Value::Object(settings.clone()), &path);
        let blocked =
            !forced && (operator_sets || project.as_ref().is_some_and(|doc| sets_path(doc, &path)));
        if !blocked {
            inserted |= insert_at(&mut settings, &path, value);
        }
    }
    inserted.then(|| {
        settings.remove("apiKeys");
        settings
    })
}

/// The leaf key paths of the teammate's Prime `agent_settings` defaults.
fn prime_leaves(teammate: &Teammate) -> Vec<(String, Value, bool)> {
    let entries = HarnessDefault::for_harness(&teammate.harness_defaults, HarnessKind::Prime)
        .filter_map(|entry| Some((entry.agent_settings.as_ref()?, entry.force.is_some())));
    default_leaves(entries)
}

/// The JSON object in `text`; None for any other text.
fn parse_object(text: &str) -> Option<serde_json::Map<String, Value>> {
    match serde_json::from_str(text) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// Whether `doc` sets the dotted `path`. A parent that is not an object
/// blocks the key too.
fn sets_path(doc: &Value, path: &str) -> bool {
    let mut node = doc;
    for key in path.split('.') {
        let Some(next) = node.get(key) else {
            return !node.is_object();
        };
        node = next;
    }
    true
}

/// Set `value` at the dotted `path`; whether it was set. A parent that is
/// not an object is left alone, and so is the value.
fn insert_at(obj: &mut serde_json::Map<String, Value>, path: &str, value: Value) -> bool {
    let mut parts: Vec<&str> = path.split('.').collect();
    let Some(key) = parts.pop() else {
        return false;
    };
    insert_at_path(obj, &parts, key, value)
}

#[cfg(unix)]
fn link(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(windows)]
fn link(target: &Path, at: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, at)
    } else {
        std::os::windows::fs::symlink_file(target, at)
    }
}

/// Build this launch's agent dir at `dir` from the operator's `src`
/// (design §6.9). Nothing is copied from `src` but a settings object with a
/// fleet key added; nothing is written into the workdir. In `src`, only a
/// missing `harness/` dir is created (empty), so the pane's global harness
/// state persists there as it does without horch.
fn build_agent_dir(src: &Path, dir: &Path, req: &PrepareRequest<'_>) -> Result<()> {
    crate::fsx::ensure_private_dir(dir)?;
    if std::fs::symlink_metadata(src.join(HARNESS_STATE)).is_err() {
        let _ = crate::fsx::ensure_private_dir(&src.join(HARNESS_STATE));
    }
    link_operator_entries(src, dir)?;
    write_or_link(
        generated_settings(src, req).map(Value::Object),
        &src.join(SETTINGS),
        &dir.join(SETTINGS),
    )?;
    write_or_link(
        fleet_models(req.compact_window, req.model),
        &src.join(MODELS),
        &dir.join(MODELS),
    )
}

/// A link in `dir` to every entry of the operator's `src` but the files a
/// launch generates, and `auth.json` always.
fn link_operator_entries(src: &Path, dir: &Path) -> Result<()> {
    let entries = std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))?;
    for entry in entries {
        let name = entry
            .with_context(|| format!("reading {}", src.display()))?
            .file_name();
        if !is_launch_private(&name) {
            let at = dir.join(&name);
            link(&src.join(&name), &at).with_context(|| format!("linking {}", at.display()))?;
        }
    }
    link_auth(src, dir)
}

/// `auth.json` is linked also when the source has none: a login in the pane
/// then lands in the operator's file, not in a dir that is removed.
fn link_auth(src: &Path, dir: &Path) -> Result<()> {
    let auth = dir.join(AUTH);
    if std::fs::symlink_metadata(&auth).is_err() {
        // Dangling when the operator has no `auth.json` yet: Prime then
        // creates nothing here and writes a login through the link.
        link(&src.join(AUTH), &auth).with_context(|| format!("linking {}", auth.display()))?;
    }
    Ok(())
}

/// A generated file, a linked lock dir (one this pane can never take), or a
/// temp file (another process's write in progress): never linked.
fn is_launch_private(name: &std::ffi::OsStr) -> bool {
    let text = name.to_string_lossy();
    name == SETTINGS || name == MODELS || text.ends_with(".lock") || text.ends_with(".tmp")
}

/// The fleet `settings.json`; None when the operator's file is linked as it
/// is. A file horch refuses to read sets everything as far as horch knows.
fn generated_settings(
    src: &Path,
    req: &PrepareRequest<'_>,
) -> Option<serde_json::Map<String, Value>> {
    let operator = crate::fsx::read_regular_bounded(&src.join(SETTINGS)).ok()?;
    let project = crate::fsx::read_regular_bounded(&project_settings(req.workdir)).ok()?;
    fleet_settings(operator.as_deref(), project.as_deref(), req.teammate)
}

/// Write `generated` to `at`; else link the operator's file when it exists.
fn write_or_link(generated: Option<Value>, operator: &Path, at: &Path) -> Result<()> {
    if let Some(value) = generated {
        let text = serde_json::to_string_pretty(&value)? + "\n";
        crate::fsx::write_atomic(at, text.as_bytes(), 0o600)?;
        Ok(())
    } else if std::fs::symlink_metadata(operator).is_ok() {
        link(operator, at).with_context(|| format!("linking {}", at.display()))
    } else {
        Ok(())
    }
}

/// The value after `--model` in a built command.
fn command_model(cmd: &Command) -> Option<String> {
    let args: Vec<_> = cmd.get_args().filter_map(|a| a.to_str()).collect();
    args.windows(2)
        .find(|w| w[0] == "--model")
        .map(|w| w[1].to_string())
}

/// The Prime Agent adapter.
pub struct Prime;

impl Harness for Prime {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Prime
    }

    /// A socket and a session directory that belong to this pane alone -
    /// otherwise `horch done` would leave its daemon running and the fleet
    /// would accumulate one per spawn.
    fn prepare(&self, ctx: &RuntimeContext, req: &PrepareRequest<'_>) -> Result<Prepared> {
        sweep_agent_dirs(&ctx.paths.state_root.join("prime"), &ctx.bins.harness.prime);
        outlive_hangup();
        let daemon = Daemon::install(&ctx.paths.state_root, &ctx.bins.harness.prime)
            .with_context(|| format!("preparing the Prime launch of {}", req.role))?;
        let mut prepared = Prepared {
            // pi-family builders append `--` before the prompt. These go into
            // the teammate's args, which come before that delimiter, or Prime
            // would treat them as prompt text.
            extra_args: vec![
                "--daemon-socket".into(),
                daemon.socket().to_string_lossy().into_owned(),
                "--session-dir".into(),
                daemon.sessions_dir().to_string_lossy().into_owned(),
            ],
            sessions_dir: Some(daemon.sessions_dir().to_path_buf()),
            ..Prepared::default()
        };
        // Without the operator's agent dir there is nothing to link, and a
        // login would land in a directory nobody keeps: Prime runs on its
        // own default dir, without a fleet window.
        let src = source_agent_dir(ctx, req.teammate);
        if src.is_dir() {
            let dir = daemon.agent_dir();
            match build_agent_dir(&src, &dir, req) {
                Ok(()) => prepared
                    .env
                    .push((AGENT_DIR_ENV.into(), dir.into_os_string())),
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&dir);
                    eprintln!(
                        "horch[{}]: Prime agent dir not built, the pane uses {}: {e:#}",
                        req.role,
                        src.display()
                    )
                }
            }
        }
        // In a herdr pane only: `horch done` finds this launch by its pane
        // and stops the daemon (`finish_pane`). Prime's built-in reporter
        // reports the pane's agent state itself.
        let pane = ctx.herdr.pane.as_ref().map(|p| p.as_str().to_string());
        if let Some(pane) = &pane {
            if let Err(e) = crate::fsx::write_atomic(
                &daemon.launch_dir().join(PANE),
                format!("{pane}\n").as_bytes(),
                crate::fsx::PRIVATE_FILE,
            ) {
                eprintln!(
                    "horch[{}]: Prime pane file not written, horch done cannot stop the daemon: {e:#}",
                    req.role
                );
            }
        }
        // After the CLI has exited, not before: stopping the daemon early would
        // take the session it is still writing with it.
        prepared.on_finish(move || daemon.finish());
        // After the daemon: its reporter sends nothing more. A clean quit
        // has released the agent already; this is for a crash. A closed
        // pane or herdr down: nothing to do.
        if let Some(pane) = pane {
            let herdr = crate::workspace::herdr::Herdr::with_bin(&ctx.bins.harness.herdr);
            prepared.on_finish(move || {
                let _ = herdr.release_agent(&pane, STATUS_SOURCE, STATUS_AGENT, release_seq());
            });
        }
        Ok(prepared)
    }

    /// The provider of the model in the operator's `models.json`, or a
    /// `compaction` setting in the agent dir's or the project's
    /// `settings.json` (design §6.4).
    fn operator_window(&self, inputs: &WindowInputs<'_>) -> Option<OperatorWindow> {
        prime_operator_window(
            &inputs.prime_agent_dir.join(MODELS),
            &[
                inputs.prime_agent_dir.join(SETTINGS),
                project_settings(inputs.workdir),
            ],
            inputs.model,
        )
    }

    /// The `contextWindow` for the command's `--model` in `models.json` of
    /// the command's `$PRIME_AGENT_CODING_AGENT_DIR`.
    fn window_in_command(&self, cmd: &Command) -> Option<u64> {
        let (_, dir) = cmd.get_envs().find(|(k, _)| *k == AGENT_DIR_ENV)?;
        let models = Path::new(dir?).join(MODELS);
        let text = crate::fsx::read_regular_bounded(&models).ok()??;
        models_window(&models.to_string_lossy(), &text, &command_model(cmd)?)?.tokens
    }

    fn expose_skills(
        &self,
        teammate: &Teammate,
        skills: &Bundle,
        _home: Option<&Path>,
    ) -> Result<Teammate> {
        Ok(super::pi::with_skill_flag(teammate, skills))
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        super::pi::pi_family_command(
            env.bins.prime.clone(),
            spec.teammate,
            spec.session,
            spec.prompt,
            spec.model_override,
        )
    }

    /// Prime writes into a directory this pane owns, so the session there is
    /// unambiguously this worker's. The resume handle is the file path, which
    /// is what `--resume` takes.
    fn discover_sessions(
        &self,
        _ctx: &RuntimeContext,
        _workdir: &Path,
        _since: SystemTime,
        sessions_dir: Option<&Path>,
    ) -> Vec<String> {
        sessions_dir
            .and_then(find_session)
            .map(|p| p.to_string_lossy().into_owned())
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = r#"[
        {"socketPath":"/tmp/prime-agent-501/daemon.sock","pid":96848,"sessionCount":0},
        {"socketPath":"/state/prime/0a1b2c3d4e5f/prime-agent-501/d.sock","pid":99865,"sessionCount":1}
    ]"#;

    /// The whole point of the per-launch socket: stopping this pane's daemon
    /// must never reach the operator's default one, or closing one worker would
    /// kill every Prime session on the machine.
    #[test]
    fn only_this_launchs_socket_matches() {
        assert_eq!(
            pid_for_socket(
                STATUS,
                Path::new("/state/prime/0a1b2c3d4e5f/prime-agent-501/d.sock")
            ),
            Some(99865)
        );
        assert_eq!(
            pid_for_socket(STATUS, Path::new("/state/prime/other/d.sock")),
            None
        );
    }

    /// A daemon that is not running yet, or output that is not what we expect,
    /// must yield nothing rather than a pid that belongs to something else.
    #[test]
    fn unmatched_or_unparseable_status_kills_nothing() {
        assert_eq!(pid_for_socket("not json", Path::new("/a/d.sock")), None);
        assert_eq!(pid_for_socket("[]", Path::new("/a/d.sock")), None);
        assert_eq!(
            pid_for_socket(r#"[{"pid":1}]"#, Path::new("/a/d.sock")),
            None
        );
        // 0.10.0: a socket file with no daemon after a clean stop.
        let orphan = r#"[{"socketPath":"/a/d.sock","status":"orphan-file","isDefault":false}]"#;
        assert_eq!(pid_for_socket(orphan, Path::new("/a/d.sock")), None);
        // -1 as a pid is every process; 0 is this group; 4294967295 turns
        // into -1.
        for pid in ["-1", "0", "4294967295"] {
            let json = format!(r#"[{{"socketPath":"/a/d.sock","pid":{pid}}}]"#);
            assert_eq!(pid_for_socket(&json, Path::new("/a/d.sock")), None, "{pid}");
        }
    }

    /// A stop counts only when the `stopped` list of `prime-agent shutdown
    /// --json` (0.10.0 shape) names this socket.
    #[test]
    fn ctx_05_prime_shutdown_counts_only_when_it_names_this_socket() {
        let socket = Path::new("/s/prime/a/prime-agent-501/d.sock");
        let stopped = r#"{"stopped":[{"socketPath":"/s/prime/a/prime-agent-501/d.sock",
            "action":"stopped background service (pid 71613)"}],"failed":[]}"#;
        assert!(stopped_names(stopped, socket));
        assert!(!stopped_names(
            stopped,
            Path::new("/s/prime/b/prime-agent-501/d.sock")
        ));
        let refused = r#"{"stopped":[],"failed":[{"socketPath":"/s/prime/a/prime-agent-501/d.sock",
            "reason":"confirmation required; use \"prime-agent shutdown --force --json\""}]}"#;
        assert!(!stopped_names(refused, socket));
        assert!(!stopped_names(r#"{"stopped":[],"failed":[]}"#, socket));
        assert!(!stopped_names("", socket));
        assert!(!stopped_names(
            r#"["/s/prime/a/prime-agent-501/d.sock"]"#,
            socket
        ));
    }

    /// SIGTERM only for one process across both `status` calls, started
    /// after this launch reserved its socket.
    #[test]
    fn only_the_same_daemon_started_by_this_launch_is_stopped() {
        use std::time::Duration;
        let installed = SystemTime::now();
        let after = Some(installed + Duration::from_secs(3));
        let ok = (7, Some(100));
        assert_eq!(may_stop(ok, Some(ok), after, installed), Ok(()));
        assert!(may_stop(ok, None, after, installed).is_err(), "gone");
        assert!(
            may_stop(ok, Some((8, Some(100))), after, installed).is_err(),
            "another pid"
        );
        let before = Some(installed - Duration::from_secs(60));
        if cfg!(windows) {
            return;
        }
        assert!(
            may_stop(ok, Some((7, Some(101))), after, installed).is_err(),
            "the pid was given to another process"
        );
        assert!(may_stop((7, None), Some((7, None)), after, installed).is_err());
        assert!(
            may_stop(ok, Some(ok), before, installed).is_err(),
            "the operator's own daemon, started earlier"
        );
        assert!(may_stop(ok, Some(ok), None, installed).is_err());
    }

    /// A temp dir with a short path: a socket path under the default temp
    /// dir of macOS can be over [`MAX_SOCKET_BYTES`].
    #[cfg(unix)]
    fn short_tempdir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("hp")
            .tempdir_in("/tmp")
            .unwrap()
    }

    /// A `prime-agent` shell script: each call appends `<command> <TMPDIR>`
    /// to the log it gives back. `shutdown` prints `shutdown_json`, `status`
    /// prints `status_json`. It runs once before it is given back, so the
    /// first-exec scan of macOS does not fall inside a test's wait.
    #[cfg(unix)]
    fn fake_bin(dir: &Path, shutdown_json: &str, status_json: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join("prime-agent");
        let log = dir.join("calls.log");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\necho \"$1 $TMPDIR\" >> '{log}'\ncase \"$1\" in\n\
                 shutdown) cat <<'JSON'\n{shutdown_json}\nJSON\n;;\n\
                 status) cat <<'JSON'\n{status_json}\nJSON\n;;\nesac\n",
                log = log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::process::Command::new(&bin)
            .arg("warm")
            .output()
            .unwrap();
        std::fs::remove_file(&log).unwrap();
        (bin, log)
    }

    #[cfg(unix)]
    fn calls(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// A socket file at `path` that nobody listens on. A child process binds
    /// it and ends, so the test process never holds the socket's fd: a
    /// listener dropped here stays alive in any process that another test
    /// forks before its `exec`, and a connect to it still succeeds.
    #[cfg(unix)]
    fn stale_socket(path: &Path) {
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: an all-zero `sockaddr_un` is a valid value.
        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let bytes = path.as_os_str().as_bytes();
        assert!(bytes.len() < addr.sun_path.len(), "{}", path.display());
        for (dst, src) in addr.sun_path.iter_mut().zip(bytes) {
            *dst = *src as libc::c_char;
        }
        // SAFETY: the child calls only `socket`, `bind` and `_exit`, which
        // are async-signal-safe; the parent waits for it.
        unsafe {
            match libc::fork() {
                -1 => panic!("fork: {}", std::io::Error::last_os_error()),
                0 => {
                    let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
                    let size = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
                    let bound = fd >= 0
                        && libc::bind(fd, (&addr as *const libc::sockaddr_un).cast(), size) == 0;
                    libc::_exit(if bound { 0 } else { 1 });
                }
                child => {
                    let mut status = 0;
                    assert_eq!(libc::waitpid(child, &mut status, 0), child);
                    assert!(libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0);
                }
            }
        }
    }

    /// `finish` stops the daemon with `prime-agent shutdown --force --json`
    /// scoped by `TMPDIR=<launch>/`, then removes the socket, its scan dir
    /// and the agent dir; the sessions stay. `status` is not asked.
    #[cfg(unix)]
    #[test]
    fn ctx_05_prime_finish_stops_with_a_scoped_shutdown() {
        let tmp = short_tempdir();
        let state = tmp.path().join("state");
        // The fake names the socket of the 1 launch this test makes.
        let probe = Daemon::install(&state, Path::new("prime-agent")).unwrap();
        let (bin, log) = fake_bin(
            tmp.path(),
            &format!(
                r#"{{"stopped":[{{"socketPath":"{}","action":"stopped"}}],"failed":[]}}"#,
                probe.socket().display()
            ),
            "[]",
        );
        let daemon = Daemon {
            bin: bin.clone(),
            ..probe
        };
        std::fs::create_dir_all(daemon.agent_dir()).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(daemon.socket()).unwrap();
        assert_eq!(daemon.stop(), None);
        drop(listener);
        assert_eq!(
            calls(&log),
            [format!("shutdown {}/", daemon.launch_dir().display())]
        );
        assert!(std::fs::symlink_metadata(daemon.socket()).is_err());
        assert!(!daemon.socket().parent().unwrap().exists(), "scan dir");
        assert!(!daemon.agent_dir().exists());
        assert!(daemon.sessions_dir().is_dir());
    }

    /// The stop fails (its `stopped` list names no socket of this launch):
    /// the `status` fallback. A daemon that started before the launch is
    /// left running with its files; one that started after it gets SIGTERM.
    /// The child sleeps 1 h, so it cannot end by itself during the test.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn ctx_05_prime_finish_stops_only_a_daemon_this_launch_started() {
        use std::os::unix::process::ExitStatusExt;
        let tmp = short_tempdir();
        let mut child = std::process::Command::new("sleep")
            .arg("3600")
            .spawn()
            .unwrap();
        let state = tmp.path().join("state");
        let probe = Daemon::install(&state, Path::new("prime-agent")).unwrap();
        let (bin, log) = fake_bin(
            tmp.path(),
            r#"{"stopped":[{"socketPath":"/elsewhere/prime-agent-501/d.sock","action":"x"}],"failed":[]}"#,
            &format!(
                r#"[{{"socketPath":"{}","pid":{}}}]"#,
                probe.socket().display(),
                child.id()
            ),
        );
        let mut daemon = Daemon { bin, ..probe };
        std::fs::create_dir_all(daemon.agent_dir()).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(daemon.socket()).unwrap();

        // Reserved long after the child started: not this launch's daemon.
        daemon.installed = SystemTime::now() + std::time::Duration::from_secs(60);
        let line = daemon.stop().expect("left running");
        assert!(line.contains(&format!("pid {}", child.id())), "{line}");
        assert!(line.contains("started before this launch"), "{line}");
        if let Some(status) = child.try_wait().unwrap() {
            panic!("a foreign daemon ended: {status:?}");
        }
        assert!(daemon.socket().exists(), "a running daemon lost its socket");
        assert!(
            daemon.agent_dir().exists(),
            "a running daemon lost its agent dir"
        );
        let tmpdir = format!("{}/", daemon.launch_dir().display());
        assert_eq!(
            calls(&log),
            [
                format!("shutdown {tmpdir}"),
                format!("status {tmpdir}"),
                format!("status {tmpdir}")
            ]
        );

        daemon.installed = SystemTime::now() - std::time::Duration::from_secs(60);
        assert_eq!(daemon.stop(), None);
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
        assert!(
            !daemon.socket().exists(),
            "a stopped daemon kept its socket"
        );
        assert!(!daemon.agent_dir().exists());
    }

    /// `status` names the socket but no pid while a process listens on it:
    /// the daemon is left running with its files, and the line says why.
    #[cfg(unix)]
    #[test]
    fn ctx_05_prime_status_with_no_pid_leaves_the_files() {
        let tmp = short_tempdir();
        let state = tmp.path().join("state");
        let probe = Daemon::install(&state, Path::new("prime-agent")).unwrap();
        let (bin, _log) = fake_bin(
            tmp.path(),
            r#"{"stopped":[],"failed":[]}"#,
            &format!(
                r#"[{{"socketPath":"{}","status":"orphan-file","isDefault":false}}]"#,
                probe.socket().display()
            ),
        );
        let daemon = Daemon { bin, ..probe };
        std::fs::create_dir_all(daemon.agent_dir()).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(daemon.socket()).unwrap();
        assert_eq!(
            daemon.stop(),
            Some(format!(
                "horch: left Prime daemon {} running: status names no pid",
                daemon.socket().display()
            ))
        );
        assert!(daemon.socket().exists());
        assert!(daemon.agent_dir().is_dir());
    }

    /// No process listens on the socket (a clean stop leaves the file; a
    /// launch that never started a daemon has none): no Prime call, and the
    /// files go.
    #[cfg(unix)]
    #[test]
    fn ctx_05_prime_finish_without_a_daemon_runs_no_stop() {
        let tmp = short_tempdir();
        let (bin, log) = fake_bin(tmp.path(), "{}", "[]");
        for stale_file in [true, false] {
            let daemon = Daemon::install(&tmp.path().join("state"), &bin).unwrap();
            std::fs::create_dir_all(daemon.agent_dir().join("daemon-workers")).unwrap();
            // A socket file nobody listens on, or no file.
            if stale_file {
                stale_socket(daemon.socket());
            }
            assert_eq!(daemon.stop(), None);
            assert!(std::fs::symlink_metadata(daemon.socket()).is_err());
            assert!(!daemon.agent_dir().exists());
        }
        assert!(calls(&log).is_empty(), "{:?}", calls(&log));
    }

    /// A socket path over 103 bytes cannot be bound on macOS: `install`
    /// fails with the path and its length, and makes no launch dir.
    #[test]
    fn ctx_05_prime_install_refuses_a_long_socket_path() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("s".repeat(90));
        let err = Daemon::install(&state, Path::new("prime-agent"))
            .unwrap_err()
            .to_string();
        let socket_len = state.as_os_str().len()
            + "/prime/".len()
            + 12
            + 1
            + scan_dir_name().len()
            + 1
            + SOCKET.len();
        assert!(err.contains(&format!("is {socket_len} bytes")), "{err}");
        assert!(err.contains(&state.display().to_string()), "{err}");
        assert!(err.contains("103"), "{err}");
        assert!(!state.exists(), "a launch dir was made");
    }

    /// `horch done` never hangs on the stop: past the limit it goes on.
    #[cfg(unix)]
    #[test]
    fn the_wait_for_the_stop_is_bounded() {
        let mut slow = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        let limit = std::time::Duration::from_millis(200);
        assert_eq!(wait_for_stop(&mut slow, limit), Waited::TimedOut);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        slow.kill().unwrap();
        slow.wait().unwrap();
        let mut ok = std::process::Command::new("true").spawn().unwrap();
        assert_eq!(wait_for_stop(&mut ok, limit * 10), Waited::Exited(true));
        let mut bad = std::process::Command::new("false").spawn().unwrap();
        assert_eq!(wait_for_stop(&mut bad, limit * 10), Waited::Exited(false));
    }

    /// A pane closing hangs up the launcher: it lives on to stop the
    /// daemon, and a program it runs after still ends on a hangup.
    #[cfg(unix)]
    #[test]
    fn the_launcher_outlives_a_hangup_and_its_child_does_not() {
        use std::os::unix::process::ExitStatusExt;
        outlive_hangup();
        // SAFETY: raising a signal that has a handler.
        assert_eq!(unsafe { libc::raise(libc::SIGHUP) }, 0);
        let status = std::process::Command::new("sh")
            .args(["-c", "kill -HUP $$; exit 0"])
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(libc::SIGHUP), "{status:?}");
    }

    /// Each launch: `<state>/prime/<12 hex>/`, the socket in its own
    /// `prime-agent-<uid>/` scan dir (mode 700), short enough to bind.
    #[test]
    fn ctx_05_prime_each_launch_reserves_its_own_socket_and_sessions() {
        let state = tempfile::tempdir().unwrap();
        let a = Daemon::install(state.path(), Path::new("prime-agent")).unwrap();
        let b = Daemon::install(state.path(), Path::new("prime-agent")).unwrap();
        assert_ne!(a.socket(), b.socket());
        assert!(a.sessions_dir().is_dir());
        let id = a.launch_dir().file_name().unwrap().to_str().unwrap();
        assert_eq!(id.len(), 12);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{id}");
        assert_eq!(a.launch_dir(), state.path().join("prime").join(id));
        assert_eq!(
            a.socket(),
            a.launch_dir()
                .join(format!("prime-agent-{}", real_uid()))
                .join("d.sock")
        );
        assert!(a.socket().parent().unwrap().is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(a.socket().parent().unwrap()), 0o700);
            assert_eq!(mode(a.launch_dir()), 0o700);
        }
    }

    /// The session file is found by being the only thing in a directory horch
    /// owns - no timestamps, no project matching, nothing to race.
    #[test]
    fn the_session_in_our_own_directory_is_ours() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_session(dir.path()).is_none());
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        assert!(find_session(dir.path()).is_none(), "only .jsonl counts");
        std::fs::write(dir.path().join("s-1.jsonl"), "{}").unwrap();
        assert_eq!(
            find_session(dir.path()).unwrap().file_name().unwrap(),
            "s-1.jsonl"
        );
    }
    // ---- Pins for the build_agent_dir and helper split (#186) ----

    fn prime_default(agent_settings: Value, force: bool) -> HarnessDefault {
        HarnessDefault {
            harness: HarnessKind::Prime,
            base: None,
            env: Default::default(),
            args: Vec::new(),
            settings: None,
            config: None,
            agent_settings: agent_settings.as_object().cloned(),
            force: force.then(|| "test".to_string()),
        }
    }

    fn with_defaults(defaults: Vec<HarnessDefault>) -> Teammate {
        Teammate {
            agent: HarnessKind::Prime,
            harness_defaults: defaults,
            ..Teammate::default()
        }
    }

    #[test]
    fn may_stop_names_the_first_rule_that_fails() {
        use std::time::Duration;
        let installed = SystemTime::now();
        let after = Some(installed + Duration::from_secs(3));
        let ok = (7, Some(100));
        assert_eq!(
            may_stop(ok, None, after, installed),
            Err("a second status call does not name it")
        );
        assert_eq!(
            may_stop(ok, Some((8, Some(100))), after, installed),
            Err("a second status call names another pid")
        );
        if cfg!(windows) {
            assert_eq!(may_stop(ok, Some((7, None)), None, installed), Ok(()));
            return;
        }
        let start_changed = "its start time is unknown or changed between 2 status calls";
        assert_eq!(
            may_stop(ok, Some((7, Some(101))), after, installed),
            Err(start_changed)
        );
        assert_eq!(
            may_stop((7, None), Some((7, None)), after, installed),
            Err(start_changed)
        );
        let early = "it started before this launch, so this launch did not start it";
        let before = Some(installed - Duration::from_secs(60));
        assert_eq!(may_stop(ok, Some(ok), before, installed), Err(early));
        assert_eq!(may_stop(ok, Some(ok), None, installed), Err(early));
        // One second of slack before the install time still counts.
        let slack = Some(installed - Duration::from_millis(900));
        assert_eq!(may_stop(ok, Some(ok), slack, installed), Ok(()));
    }

    #[test]
    fn fleet_settings_adds_defaults_the_operator_and_project_do_not_set() {
        let t = with_defaults(vec![
            prime_default(json!({"a": 1, "b": {"c": 2, "d": 3}, "e": 5}), false),
            prime_default(json!({"b": {"c": 9}, "f": {"g": true}}), true),
        ]);
        let operator = r#"{"apiKeys":{"x":"secret"},"a":0,"b":{"d":4},"e":{"k":1}}"#;
        let project = r#"{"f":{"other":1}}"#;
        let got = fleet_settings(Some(operator), Some(project), &t).unwrap();
        assert_eq!(
            Value::Object(got),
            json!({
                "a": 0,
                "b": {"c": 9, "d": 4},
                "e": {"k": 1},
                "f": {"g": true}
            })
        );

        // The project's key blocks a non-forced default.
        let t = with_defaults(vec![prime_default(json!({"p": 1}), false)]);
        assert_eq!(
            fleet_settings(None, Some(r#"{"p":0}"#), &t),
            None,
            "the project sets it"
        );
        let got = fleet_settings(None, Some("not json"), &t).unwrap();
        assert_eq!(Value::Object(got), json!({"p": 1}));
        let got = fleet_settings(None, None, &t).unwrap();
        assert_eq!(Value::Object(got), json!({"p": 1}));
    }

    #[test]
    fn fleet_settings_leaves_the_operators_file_alone_when_it_adds_nothing() {
        let t = with_defaults(vec![prime_default(json!({"a": 1}), false)]);
        // Nothing to add: no copy, so `apiKeys` is not stripped either.
        assert_eq!(
            fleet_settings(Some(r#"{"a":0,"apiKeys":{"x":"y"}}"#), None, &t),
            None
        );
        // Not an object: linked as it is.
        for text in ["[]", "not json", "3"] {
            assert_eq!(fleet_settings(Some(text), None, &t), None, "{text}");
        }
        // No defaults at all.
        assert_eq!(
            fleet_settings(Some("{}"), None, &with_defaults(vec![])),
            None
        );
        // A parent that is not an object blocks the key, forced or not.
        for force in [false, true] {
            let t = with_defaults(vec![prime_default(json!({"a": {"b": 1}}), force)]);
            assert_eq!(
                fleet_settings(Some(r#"{"a":3}"#), None, &t),
                None,
                "{force}"
            );
        }
        // A default for another harness is ignored.
        let mut other = prime_default(json!({"z": 1}), true);
        other.harness = HarnessKind::Pi;
        assert_eq!(
            fleet_settings(None, None, &with_defaults(vec![other])),
            None
        );
    }

    #[test]
    fn insert_at_sets_nested_keys_and_refuses_a_non_object_parent() {
        let mut obj = serde_json::Map::new();
        assert!(insert_at(&mut obj, "a.b.c", json!(1)));
        assert_eq!(Value::Object(obj.clone()), json!({"a": {"b": {"c": 1}}}));
        assert!(insert_at(&mut obj, "a.b.d", json!(2)));
        assert_eq!(obj["a"]["b"], json!({"c": 1, "d": 2}));
        assert!(insert_at(&mut obj, "top", json!(true)));
        let mut blocked = serde_json::Map::new();
        blocked.insert("a".into(), json!(3));
        assert!(!insert_at(&mut blocked, "a.b", json!(1)));
        assert_eq!(Value::Object(blocked), json!({"a": 3}));
    }

    #[test]
    fn the_operator_window_comes_from_models_then_settings() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models.json");
        let user = dir.path().join("settings.json");
        let project = dir.path().join("project.json");
        let window =
            |model: &str| prime_operator_window(&models, &[user.clone(), project.clone()], model);

        assert_eq!(window("p/m"), None, "no files");

        std::fs::write(
            &models,
            r#"{"providers":{"p":{"modelOverrides":{"m":{"contextWindow":1000}},"models":[{"id":"x","contextWindow":2000}]}}}"#,
        )
        .unwrap();
        let w = window("p/m").unwrap();
        assert_eq!(w.tokens, Some(1000));
        assert_eq!(
            w.detail,
            format!("{} (provider p defined)", models.display())
        );
        assert_eq!(window("p/x").unwrap().tokens, Some(2000));
        assert_eq!(window("p/none").unwrap().tokens, None);
        assert_eq!(window("other/m"), None, "provider not defined");
        assert_eq!(window("no-slash"), None);

        std::fs::write(&models, "{oops").unwrap();
        let w = window("p/m").unwrap();
        assert_eq!(
            (w.tokens, w.detail),
            (None, format!("{} (not valid JSON)", models.display()))
        );

        std::fs::remove_file(&models).unwrap();
        std::fs::write(&user, r#"{"theme":"x","compaction":{"keepRecent":1}}"#).unwrap();
        assert_eq!(window("p/m"), None, "compaction without the trigger keys");
        std::fs::write(&project, r#"{"compaction":{"enabled":false}}"#).unwrap();
        let w = window("p/m").unwrap();
        assert_eq!(w.tokens, None);
        assert_eq!(
            w.detail,
            format!("{} (compaction set; trigger unknown)", project.display())
        );
        std::fs::write(&user, r#"{"compaction":{"reserveTokens":5}}"#).unwrap();
        assert!(window("p/m")
            .unwrap()
            .detail
            .starts_with(&user.display().to_string()));
    }

    #[test]
    fn find_session_picks_the_newest_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let at = |name: &str, secs: u64| {
            let path = dir.path().join(name);
            std::fs::write(&path, "{}").unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs))
                .unwrap();
        };
        at("old.jsonl", 1000);
        at("new.jsonl", 3000);
        at("mid.jsonl", 2000);
        at("newest.txt", 9000);
        assert_eq!(
            find_session(dir.path()).unwrap().file_name().unwrap(),
            "new.jsonl"
        );
        assert!(find_session(&dir.path().join("missing")).is_none());
    }

    #[test]
    fn the_launcher_file_names_a_live_horch_or_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let launch = dir.path();
        assert!(!launcher_alive(launch), "no file");
        std::fs::write(launch.join(LAUNCHER), "not-a-pid 1\n").unwrap();
        assert!(!launcher_alive(launch), "garbage");
        std::fs::write(launch.join(LAUNCHER), "").unwrap();
        assert!(!launcher_alive(launch), "empty");

        let pid = std::process::id();
        let started = crate::procid::start_time(pid)
            .map(|t| t.to_string())
            .unwrap_or_default();
        std::fs::write(launch.join(LAUNCHER), format!("{pid} {started}\n")).unwrap();
        assert!(launcher_alive(launch), "this process");
        if !started.is_empty() {
            std::fs::write(launch.join(LAUNCHER), format!("{pid} 1\n")).unwrap();
            assert!(!launcher_alive(launch), "same pid, another start time");
        }
    }

    #[test]
    fn finish_pane_does_nothing_for_a_pane_that_ran_no_launch() {
        let state = tempfile::tempdir().unwrap();
        // No prime dir at all.
        finish_pane(state.path(), Path::new("prime-agent"), "p1");
        // A launch of another pane, with a socket file: it stays.
        let launch = state.path().join("prime/aaaaaaaaaaaa");
        std::fs::create_dir_all(launch.join(scan_dir_name())).unwrap();
        std::fs::write(launch.join(PANE), "other\n").unwrap();
        std::fs::write(launch.join(scan_dir_name()).join(SOCKET), "").unwrap();
        finish_pane(state.path(), Path::new("prime-agent"), "p1");
        assert!(launch.join(scan_dir_name()).join(SOCKET).exists());
        // This pane's launch, with no launcher file: its daemon is kept.
        std::fs::write(launch.join(PANE), "p1\n").unwrap();
        finish_pane(state.path(), Path::new("prime-agent"), "p1");
        assert!(launch.join(scan_dir_name()).join(SOCKET).exists());
    }
}
