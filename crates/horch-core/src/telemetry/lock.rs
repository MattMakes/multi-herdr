//! The collector singleton (section 12.2, SPC-01).
//!
//! `telemetry/collector.lock/` is created with `create_dir`, which is atomic,
//! the same idiom as `Ledger::lock`. The holder writes `collector.json` next
//! to it. A lock whose pid is dead is broken, and the breaker says so.
//!
//! Liveness is "the pid exists" (`kill(pid, 0)` on Unix, `tasklist` on
//! Windows). The design also asks that the process start time match; stable
//! Rust has no portable way to read it, so a reused pid would read as live
//! until that process exits. The lock is also removed on every normal exit.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub started_at: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub herdr_session: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub pane_id: Option<String>,
}

pub fn lock_dir(state_root: &Path) -> PathBuf {
    super::dir(state_root).join("collector.lock")
}

pub fn info_path(state_root: &Path) -> PathBuf {
    super::dir(state_root).join("collector.json")
}

/// Whether process `pid` exists.
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        // Signal 0 checks existence and permission without sending anything.
        let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
        r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}

/// Who holds the lock, if anyone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    Free,
    Live(LockInfo),
    /// The lock exists but its holder is gone (or never wrote its info).
    Stale(Option<LockInfo>),
}

pub fn read_info(state_root: &Path) -> Option<LockInfo> {
    std::fs::read_to_string(info_path(state_root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

pub fn holder(state_root: &Path) -> Holder {
    if !lock_dir(state_root).is_dir() {
        return Holder::Free;
    }
    match read_info(state_root) {
        Some(info) if pid_alive(info.pid) => Holder::Live(info),
        Some(info) => Holder::Stale(Some(info)),
        None => {
            // A holder between `create_dir` and writing its info is live for
            // a moment; only an old, empty lock is stale.
            let young = std::fs::metadata(lock_dir(state_root))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() < 5);
            if young {
                Holder::Live(LockInfo::default())
            } else {
                Holder::Stale(None)
            }
        }
    }
}

/// Whether a live collector holds the lock.
pub fn collector_live(state_root: &Path) -> bool {
    matches!(holder(state_root), Holder::Live(_))
}

/// The held lock. Dropping it releases the lock.
#[derive(Debug)]
pub struct CollectorLock {
    dir: PathBuf,
    info: PathBuf,
}

impl Drop for CollectorLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.info);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl CollectorLock {
    /// Release now (the same as dropping it).
    pub fn release(self) {}
}

/// Take the lock, breaking a stale one. `Ok(Err(info))` when a live
/// collector holds it.
pub fn acquire(
    state_root: &Path,
    info: &LockInfo,
) -> Result<std::result::Result<CollectorLock, LockInfo>> {
    let dir = lock_dir(state_root);
    std::fs::create_dir_all(super::dir(state_root))
        .with_context(|| format!("creating {}", super::dir(state_root).display()))?;
    for _ in 0..2 {
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                let json = serde_json::to_vec_pretty(info)?;
                super::store::replace_private(&info_path(state_root), &json)?;
                return Ok(Ok(CollectorLock {
                    dir,
                    info: info_path(state_root),
                }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => match holder(state_root) {
                Holder::Live(other) => return Ok(Err(other)),
                Holder::Stale(old) => {
                    match &old {
                        Some(o) => {
                            eprintln!("horch: removed stale telemetry lock of pid {}", o.pid)
                        }
                        None => {
                            eprintln!("horch: removed stale telemetry lock (no holder recorded)")
                        }
                    }
                    let _ = std::fs::remove_file(info_path(state_root));
                    let _ = std::fs::remove_dir_all(&dir);
                }
                Holder::Free => {}
            },
            Err(e) => return Err(e).with_context(|| format!("creating {}", dir.display())),
        }
    }
    // Lost a race twice: someone else holds it now.
    Ok(Err(read_info(state_root).unwrap_or_default()))
}

/// This process's lock info.
pub fn this_process(now: &str) -> LockInfo {
    let env = |k: &str| std::env::var(k).ok().filter(|s| !s.is_empty());
    LockInfo {
        pid: std::process::id(),
        started_at: now.to_string(),
        host: env("HOSTNAME")
            .or_else(|| env("COMPUTERNAME"))
            .unwrap_or_default(),
        herdr_session: env("HERDR_SESSION"),
        workspace_id: env("HORCH_WORKSPACE_ID"),
        pane_id: env("HERDR_PANE_ID"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spc_01_second_collector_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let me = this_process("2026-09-28T18:00:00Z");
        let held = acquire(tmp.path(), &me).unwrap().expect("first taker wins");
        let second = acquire(tmp.path(), &me).unwrap();
        assert_eq!(second.unwrap_err().pid, std::process::id());
        assert!(collector_live(tmp.path()));
        drop(held);
        assert_eq!(holder(tmp.path()), Holder::Free);
        assert!(
            acquire(tmp.path(), &me).unwrap().is_ok(),
            "released on drop"
        );
    }

    #[test]
    fn spc_01_stale_lock_is_broken() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(lock_dir(tmp.path())).unwrap();
        // A pid that cannot be running: beyond every platform's pid range.
        let dead = LockInfo {
            pid: 999_999_999,
            started_at: "2026-09-28T17:00:00Z".into(),
            ..LockInfo::default()
        };
        std::fs::write(info_path(tmp.path()), serde_json::to_string(&dead).unwrap()).unwrap();
        assert!(matches!(holder(tmp.path()), Holder::Stale(Some(_))));
        let lock = acquire(tmp.path(), &this_process("now")).unwrap();
        assert!(lock.is_ok(), "the stale lock is broken and taken");
        assert_eq!(read_info(tmp.path()).unwrap().pid, std::process::id());
    }
}
