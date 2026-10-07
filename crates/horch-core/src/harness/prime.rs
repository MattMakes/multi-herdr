//! Prime Agent-specific glue: containing its daemon, and finding the session it
//! wrote.
//!
//! Prime Agent supervises its own agents. Verified on 0.9.4: even a `--print`
//! run that failed authentication left a background service alive after the
//! process exited. herdr already supervises - a pane IS the agent's lifetime -
//! so left alone, `horch done` closing a worker's pane would leave that worker's
//! Prime daemon running, and a fleet would accumulate one per spawn.
//!
//! `--daemon-socket` is the containment: each launch gets its own socket, so its
//! daemon is its own process rather than a shared one. `prime-agent status
//! --json` maps a socket path back to a pid, which is how [`Daemon::finish`]
//! stops exactly this pane's service and leaves the operator's own alone -
//! `prime-agent shutdown` cannot be scoped and would stop every agent on the
//! machine, including other panes in the same fleet.
//!
//! Sessions are the other half. Prime has no `--session-id` (pi does), so horch
//! cannot name the session before launch. It can own the directory: with
//! `--session-dir` pointing somewhere only this launch writes, the session file
//! that appears there IS this worker's, with no timestamps or project paths to
//! disambiguate.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result};

use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, PrepareRequest, Prepared};
use crate::roster::Teammate;
use crate::runtime::RuntimeContext;
use crate::skills::Bundle;

/// One Prime Agent launch's private daemon socket and session directory.
#[derive(Debug, Clone)]
pub struct Daemon {
    socket: PathBuf,
    sessions: PathBuf,
    /// The `prime-agent` program that answers `status` for [`Daemon::finish`].
    bin: PathBuf,
    /// When this launch reserved its socket. Its daemon starts later; a
    /// process that started earlier is not it.
    installed: SystemTime,
}

impl Daemon {
    /// Reserve a socket and session directory for the pane running `role`.
    /// `bin` is the `prime-agent` program the launch runs.
    pub fn install(state_root: &Path, role: &str, bin: &Path) -> Result<Daemon> {
        let slug: String = role
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let base = state_root
            .join("prime")
            .join(format!("{slug}-{}", crate::mint_uuid()));
        let sessions = base.join("sessions");
        std::fs::create_dir_all(&sessions)
            .with_context(|| format!("creating {}", sessions.display()))?;
        Ok(Daemon {
            // Unix sockets have a path length limit (~104 bytes on macOS), so
            // this stays a short name in an already-short state directory.
            socket: base.join("d.sock"),
            sessions,
            bin: bin.to_path_buf(),
            installed: SystemTime::now(),
        })
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions
    }

    /// Stop this launch's daemon, and only this one.
    ///
    /// `status` names the pid; a pid can be given to another program once
    /// its process ends. So `status` is asked twice, and SIGTERM goes only
    /// to a pid that both answers name with the same start time, and that
    /// started after [`Daemon::install`]: horch never stops a daemon it did
    /// not start, such as the operator's own. Otherwise the daemon is left
    /// running, and the reason is logged. A daemon left running keeps its
    /// socket.
    ///
    /// Residual risk (U-63, checked on prime-agent 0.9.4,
    /// docs/live-checks/harnesses.md): Prime reports no start time of its
    /// own. `status --json` gives `pid`, `socketPath` and `uptimeSeconds`,
    /// and the uptime is `ps -o etimes` of that same pid, so it adds nothing
    /// to [`crate::procid`]. Prime takes the pid from `ss -lxp` or `lsof`:
    /// a `prime-agent` process that holds the socket. So a wrong SIGTERM
    /// needs a `prime-agent` process that started after this launch, got the
    /// dead daemon's pid and holds this launch's private socket.
    pub fn finish(&self) {
        let left_running = daemon_pid(&self.bin, &self.socket).is_some_and(|pid| {
            let first = pid_start(pid);
            let again = daemon_pid(&self.bin, &self.socket).map(|p| (p, pid_start(p)));
            let started_at = u32::try_from(pid).ok().and_then(crate::procid::started_at);
            match may_stop((pid, first), again, started_at, self.installed) {
                Ok(()) => {
                    terminate(pid, first);
                    false
                }
                Err(why) => {
                    eprintln!("horch: left Prime daemon pid {pid} running: {why}");
                    true
                }
            }
        });
        if !left_running {
            let _ = std::fs::remove_file(&self.socket);
        }
    }
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
    let Some(again) = again else {
        return Err("a second status call does not name it");
    };
    if again.0 != first.0 {
        return Err("a second status call names another pid");
    }
    if cfg!(windows) {
        // No start times here: the pid is all there is (see `procid`).
        return Ok(());
    }
    if first.1.is_none() || again.1 != first.1 {
        return Err("its start time is unknown or changed between 2 status calls");
    }
    match started_at {
        Some(t) if t + START_SLACK >= installed => Ok(()),
        _ => Err("it started before this launch, so this launch did not start it"),
    }
}

/// The pid of the Prime daemon listening on `socket`, from `prime-agent status`.
fn daemon_pid(bin: &Path, socket: &Path) -> Option<i32> {
    let output = Command::new(bin).args(["status", "--json"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    pid_for_socket(&String::from_utf8_lossy(&output.stdout), socket)
}

/// Split out from [`daemon_pid`] so the matching is testable without Prime
/// installed.
pub(crate) fn pid_for_socket(json: &str, socket: &Path) -> Option<i32> {
    let records: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    records
        .iter()
        .find_map(|r| {
            let path = r.get("socketPath")?.as_str()?;
            (Path::new(path) == socket)
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
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(sessions_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(t, _)| modified > *t) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, p)| p)
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
        let daemon = Daemon::install(&ctx.paths.state_root, req.role, &ctx.bins.harness.prime)?;
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
        // After the CLI has exited, not before: stopping the daemon early would
        // take the session it is still writing with it.
        prepared.on_finish(move || daemon.finish());
        Ok(prepared)
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
        {"socketPath":"/state/prime/codex-1-abc/d.sock","pid":99865,"sessionCount":1}
    ]"#;

    /// The whole point of the per-launch socket: stopping this pane's daemon
    /// must never reach the operator's default one, or closing one worker would
    /// kill every Prime session on the machine.
    #[test]
    fn only_this_launchs_socket_matches() {
        assert_eq!(
            pid_for_socket(STATUS, Path::new("/state/prime/codex-1-abc/d.sock")),
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
        // -1 as a pid is every process; 0 is this group; 4294967295 turns
        // into -1.
        for pid in ["-1", "0", "4294967295"] {
            let json = format!(r#"[{{"socketPath":"/a/d.sock","pid":{pid}}}]"#);
            assert_eq!(pid_for_socket(&json, Path::new("/a/d.sock")), None, "{pid}");
        }
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

    /// `finish` against a `status` program that names a child of this test:
    /// a child started before the launch is left running; one started after
    /// it gets SIGTERM.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn finish_stops_only_a_daemon_this_launch_started() {
        use std::os::unix::fs::PermissionsExt;
        let state = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let bin = state.path().join("prime-agent");
        let mut daemon = Daemon::install(state.path(), "prime-1", &bin).unwrap();
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\necho '[{{\"socketPath\":\"{}\",\"pid\":{}}}]'\n",
                daemon.socket().display(),
                child.id()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Reserved long after the child started: not this launch's daemon.
        // The daemon's socket; a plain file does for this check.
        std::fs::write(daemon.socket(), "").unwrap();
        daemon.installed = SystemTime::now() + std::time::Duration::from_secs(60);
        daemon.finish();
        assert_eq!(
            child.try_wait().unwrap(),
            None,
            "a foreign daemon was stopped"
        );
        assert!(daemon.socket().exists(), "a running daemon lost its socket");

        daemon.installed = SystemTime::now() - std::time::Duration::from_secs(60);
        daemon.finish();
        let status = child.wait().unwrap();
        assert!(!status.success(), "this launch's daemon was not stopped");
        assert!(
            !daemon.socket().exists(),
            "a stopped daemon kept its socket"
        );
    }

    #[test]
    fn each_launch_reserves_its_own_socket_and_sessions() {
        let state = tempfile::tempdir().unwrap();
        let a = Daemon::install(state.path(), "prime-1", Path::new("prime-agent")).unwrap();
        let b = Daemon::install(state.path(), "prime-1", Path::new("prime-agent")).unwrap();
        assert_ne!(a.socket(), b.socket());
        assert!(a.sessions_dir().is_dir());
        assert!(a.socket().to_string_lossy().contains("prime-1"));
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
}
