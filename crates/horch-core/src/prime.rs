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

use anyhow::{Context, Result};

use crate::agent;

/// One Prime Agent launch's private daemon socket and session directory.
#[derive(Debug, Clone)]
pub struct Daemon {
    socket: PathBuf,
    sessions: PathBuf,
}

impl Daemon {
    /// Reserve a socket and session directory for the pane running `role`.
    pub fn install(state_root: &Path, role: &str) -> Result<Daemon> {
        let slug: String = role
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
            .collect();
        let base = state_root.join("prime").join(format!("{slug}-{}", crate::mint_uuid()));
        let sessions = base.join("sessions");
        std::fs::create_dir_all(&sessions)
            .with_context(|| format!("creating {}", sessions.display()))?;
        Ok(Daemon {
            // Unix sockets have a path length limit (~104 bytes on macOS), so
            // this stays a short name in an already-short state directory.
            socket: base.join("d.sock"),
            sessions,
        })
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions
    }

    /// Stop this launch's daemon, and only this one.
    pub fn finish(&self) {
        if let Some(pid) = daemon_pid(&self.socket) {
            terminate(pid);
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// The pid of the Prime daemon listening on `socket`, from `prime-agent status`.
fn daemon_pid(socket: &Path) -> Option<i32> {
    let output = Command::new(agent::prime_bin())
        .args(["status", "--json"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    pid_for_socket(&String::from_utf8_lossy(&output.stdout), socket)
}

/// Split out from [`daemon_pid`] so the matching is testable without Prime
/// installed.
pub fn pid_for_socket(json: &str, socket: &Path) -> Option<i32> {
    let records: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    records.iter().find_map(|r| {
        let path = r.get("socketPath")?.as_str()?;
        (Path::new(path) == socket).then(|| r.get("pid")?.as_i64()).flatten()
    }).map(|pid| pid as i32)
}

#[cfg(unix)]
fn terminate(pid: i32) {
    // SIGTERM, not SIGKILL: the daemon flushes its sessions on the way out, and
    // a session file half-written is a resume that fails later in a pane nobody
    // is watching.
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
}

#[cfg(not(unix))]
fn terminate(pid: i32) {
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
        assert_eq!(pid_for_socket(STATUS, Path::new("/state/prime/other/d.sock")), None);
    }

    /// A daemon that is not running yet, or output that is not what we expect,
    /// must yield nothing rather than a pid that belongs to something else.
    #[test]
    fn unmatched_or_unparseable_status_kills_nothing() {
        assert_eq!(pid_for_socket("not json", Path::new("/a/d.sock")), None);
        assert_eq!(pid_for_socket("[]", Path::new("/a/d.sock")), None);
        assert_eq!(pid_for_socket(r#"[{"pid":1}]"#, Path::new("/a/d.sock")), None);
    }

    #[test]
    fn each_launch_reserves_its_own_socket_and_sessions() {
        let state = tempfile::tempdir().unwrap();
        let a = Daemon::install(state.path(), "prime-1").unwrap();
        let b = Daemon::install(state.path(), "prime-1").unwrap();
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
