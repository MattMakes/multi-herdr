//! The collector singleton (section 12.2, SPC-01).
//!
//! `telemetry/collector.lock/` is created with `create_dir`, which is atomic,
//! the same idiom as `Ledger::lock`. The holder writes `collector.json` next
//! to it. A lock whose pid is dead is broken, and the breaker says so.
//!
//! Liveness is "the pid exists and has the recorded start time"
//! ([`crate::procid::alive`]), so a reused pid does not keep a stale lock
//! alive. A `collector.json` without a start time (written before it
//! existed), or a platform without start times (Windows), falls back to
//! "the pid exists". The lock is also removed on every normal exit.
//!
//! The holder also records its binary ([`ExeIdentity`]: path, size and
//! modification time). A collector keeps running the binary it started
//! with, so after `just install` replaces that file it runs old code: old
//! prices, old readers. [`runs_other_binary`] finds it, and [`stop`] ends
//! it, by pid and start time only, so `horch telemetry ensure` and
//! `horch install` can start a current one.

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
    /// The holder's [`crate::procid::start_time`]; `None` in a file written
    /// before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid_start: Option<u64>,
    /// The binary the holder runs; `None` in a file written before it
    /// existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe: Option<ExeIdentity>,
}

/// A binary file as it is now: a rebuilt or reinstalled binary at the same
/// path has another size or modification time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExeIdentity {
    /// The path, symlinks resolved when they resolve.
    pub path: String,
    pub len: u64,
    /// Nanoseconds since the Unix epoch.
    pub mtime_ns: u64,
}

/// The identity of the file at `path` now, or `None` when it cannot be read.
pub fn exe_identity(path: &Path) -> Option<ExeIdentity> {
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let meta = std::fs::metadata(&real).ok()?;
    let mtime = meta.modified().ok()?;
    let mtime_ns = mtime.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    Some(ExeIdentity {
        path: real.to_string_lossy().into_owned(),
        len: meta.len(),
        mtime_ns: u64::try_from(mtime_ns).ok()?,
    })
}

/// Whether the live holder `info` runs another binary than `current`, the
/// binary that should collect now. A holder that recorded no binary was
/// written by a horch older than this check, so it is not current either.
/// With `current` unknown, or a holder without a pid (a lock being taken
/// right now), nothing can be told: `false`.
pub fn runs_other_binary(info: &LockInfo, current: Option<&ExeIdentity>) -> bool {
    match current {
        Some(want) if info.pid != 0 => info.exe.as_ref() != Some(want),
        _ => false,
    }
}

/// Stop the live holder `info` with SIGTERM, then wait up to `wait` for it
/// to give up the lock. The signal goes only to the pid that still has the
/// recorded start time ([`procid::signal_same`]): never to a holder without
/// a start time, and never to a process that got the pid later.
#[cfg(unix)]
pub fn stop(state_root: &Path, info: &LockInfo, wait: std::time::Duration) -> Result<()> {
    let Some(started) = info.pid_start else {
        anyhow::bail!(
            "collector pid {} recorded no start time, so horch cannot prove it is the collector; stop it by hand",
            info.pid
        );
    };
    if !procid::signal_same(info.pid, started, libc::SIGTERM) {
        anyhow::bail!(
            "collector pid {} no longer runs with its recorded start time; nothing was signalled",
            info.pid
        );
    }
    let until = std::time::Instant::now() + wait;
    while std::time::Instant::now() < until {
        let gone = !procid::alive(info.pid, Some(started));
        let released = !matches!(holder(state_root), Holder::Live(ref i) if i.pid == info.pid);
        if gone || released {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    anyhow::bail!(
        "collector pid {} did not stop within {}s of SIGTERM",
        info.pid,
        wait.as_secs()
    )
}

/// No signals here: a stale collector is reported, not stopped.
#[cfg(not(unix))]
pub fn stop(_state_root: &Path, info: &LockInfo, _wait: std::time::Duration) -> Result<()> {
    anyhow::bail!(
        "collector pid {} runs an older binary; stop it by hand on this platform",
        info.pid
    )
}

pub(crate) fn lock_dir(state_root: &Path) -> PathBuf {
    super::dir(state_root).join("collector.lock")
}

pub(crate) fn info_path(state_root: &Path) -> PathBuf {
    super::dir(state_root).join("collector.json")
}

use crate::procid;

/// Who holds the lock, if anyone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    Free,
    Live(LockInfo),
    /// The lock exists but its holder is gone (or never wrote its info).
    Stale(Option<LockInfo>),
}

pub(crate) fn read_info(state_root: &Path) -> Option<LockInfo> {
    std::fs::read_to_string(info_path(state_root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

pub fn holder(state_root: &Path) -> Holder {
    if !lock_dir(state_root).is_dir() {
        return Holder::Free;
    }
    match read_info(state_root) {
        Some(info) if procid::alive(info.pid, info.pid_start) => Holder::Live(info),
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

/// This process's lock info: its host, herdr session, workspace and pane
/// come from the context.
pub fn this_process(now: &str, ctx: &crate::runtime::RuntimeContext) -> LockInfo {
    LockInfo {
        pid: std::process::id(),
        started_at: now.to_string(),
        host: ctx.inherited.hostname.clone().unwrap_or_default(),
        herdr_session: ctx.inherited.herdr_session.clone(),
        workspace_id: ctx.herdr.workspace.as_ref().map(|w| w.to_string()),
        pane_id: ctx.herdr.pane.as_ref().map(|p| p.to_string()),
        pid_start: procid::start_time(std::process::id()),
        exe: ctx.bins.current_exe.as_deref().and_then(exe_identity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> crate::runtime::RuntimeContext {
        crate::runtime::RuntimeContext::from_env(&crate::runtime::MapEnv::new("/")).unwrap()
    }

    #[test]
    fn spc_01_second_collector_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let me = this_process("2026-09-28T18:00:00Z", &test_ctx());
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
        let lock = acquire(tmp.path(), &this_process("now", &test_ctx())).unwrap();
        assert!(lock.is_ok(), "the stale lock is broken and taken");
        assert_eq!(read_info(tmp.path()).unwrap().pid, std::process::id());
    }

    /// A pid that names no single process (it would turn into -1, every
    /// process, as a `pid_t`) is a dead holder.
    #[test]
    fn a_pid_out_of_range_is_a_stale_lock() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(lock_dir(tmp.path())).unwrap();
        for pid in [u32::MAX, 0] {
            let info = LockInfo {
                pid,
                started_at: "2026-09-28T17:00:00Z".into(),
                ..LockInfo::default()
            };
            std::fs::write(info_path(tmp.path()), serde_json::to_string(&info).unwrap()).unwrap();
            assert!(
                matches!(holder(tmp.path()), Holder::Stale(Some(_))),
                "pid {pid}"
            );
        }
    }

    /// W9: a collector that recorded another binary, or no binary at all
    /// (a horch older than the record), runs other code than `current`.
    #[test]
    fn a_collector_on_another_binary_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("horch");
        std::fs::write(&bin, b"old build").unwrap();
        let old = exe_identity(&bin).unwrap();
        let info = LockInfo {
            pid: 42,
            exe: Some(old.clone()),
            ..LockInfo::default()
        };
        assert!(!runs_other_binary(&info, Some(&old)), "the same binary");
        // `just install` replaces the file: another size and mtime.
        std::fs::write(&bin, b"a newer, longer build").unwrap();
        let new = exe_identity(&bin).unwrap();
        assert_ne!(new, old);
        assert!(runs_other_binary(&info, Some(&new)));
        let unrecorded = LockInfo {
            exe: None,
            ..info.clone()
        };
        assert!(runs_other_binary(&unrecorded, Some(&new)), "an older horch");
        assert!(!runs_other_binary(&info, None), "nothing to compare");
        let taking = LockInfo {
            pid: 0,
            ..info.clone()
        };
        assert!(
            !runs_other_binary(&taking, Some(&new)),
            "a lock being taken"
        );
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(serde_json::from_str::<LockInfo>(&json).unwrap(), info);
        assert!(serde_json::from_str::<LockInfo>(r#"{"pid":1,"started_at":"x"}"#).is_ok());
    }

    /// A fake collector: a child process recorded as the lock holder, with
    /// a thread that reaps it so its pid goes away when it ends.
    #[cfg(unix)]
    fn fake_collector(
        root: &Path,
        pid_start: impl Fn(u32) -> Option<u64>,
    ) -> (LockInfo, std::sync::mpsc::Receiver<()>) {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = child.wait();
            let _ = tx.send(());
        });
        std::fs::create_dir_all(lock_dir(root)).unwrap();
        let info = LockInfo {
            pid,
            started_at: "2026-10-04T15:39:46Z".into(),
            pid_start: pid_start(pid),
            exe: None,
            ..LockInfo::default()
        };
        std::fs::write(info_path(root), serde_json::to_string(&info).unwrap()).unwrap();
        (info, rx)
    }

    /// W9: `stop` ends the recorded collector, by pid and start time.
    #[cfg(unix)]
    #[test]
    fn stop_ends_the_recorded_collector() {
        let tmp = tempfile::tempdir().unwrap();
        let (info, ended) = fake_collector(tmp.path(), procid::start_time);
        if info.pid_start.is_none() {
            return; // No start times on this platform.
        }
        assert!(matches!(holder(tmp.path()), Holder::Live(_)));
        stop(tmp.path(), &info, std::time::Duration::from_secs(5)).unwrap();
        ended
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the fake collector ended");
        assert!(matches!(holder(tmp.path()), Holder::Stale(_)));
    }

    /// W9: never a signal to a process that is not the recorded collector:
    /// not to a pid whose start time differs, not to a holder that recorded
    /// no start time.
    #[cfg(unix)]
    #[test]
    fn stop_never_signals_a_process_that_is_not_the_recorded_collector() {
        let tmp = tempfile::tempdir().unwrap();
        for pid_start in [
            (|pid| procid::start_time(pid).map(|t| t + 1)) as fn(u32) -> Option<u64>,
            |_| None,
        ] {
            let (info, ended) = fake_collector(tmp.path(), pid_start);
            if procid::start_time(info.pid).is_none() {
                return;
            }
            let err = stop(tmp.path(), &info, std::time::Duration::from_millis(200));
            assert!(err.is_err(), "{info:?}");
            assert!(
                ended
                    .recv_timeout(std::time::Duration::from_millis(300))
                    .is_err(),
                "the process still runs"
            );
            let os = procid::os_pid(info.pid).unwrap();
            // SAFETY: our own child, still unreaped by its waiter.
            unsafe { libc::kill(os, libc::SIGKILL) };
            ended
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
    }

    /// A lock whose pid runs with another start time is stale: the pid was
    /// given to a later program. Without a start time (an old file), the
    /// pid alone decides, as before.
    #[test]
    fn spc_01_a_reused_pid_is_a_stale_lock() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(lock_dir(tmp.path())).unwrap();
        let me = std::process::id();
        let info = |pid_start| LockInfo {
            pid: me,
            started_at: "2026-09-28T17:00:00Z".into(),
            pid_start,
            ..LockInfo::default()
        };
        let put = |i: &LockInfo| {
            std::fs::write(info_path(tmp.path()), serde_json::to_string(i).unwrap()).unwrap()
        };
        std::fs::write(
            info_path(tmp.path()),
            format!(r#"{{"pid":{me},"started_at":"x"}}"#),
        )
        .unwrap();
        assert!(matches!(holder(tmp.path()), Holder::Live(_)), "an old file");
        let Some(mine) = procid::start_time(me) else {
            return; // No start times on this platform.
        };
        put(&info(Some(mine)));
        assert!(matches!(holder(tmp.path()), Holder::Live(_)));
        put(&info(Some(mine + 1)));
        assert!(matches!(holder(tmp.path()), Holder::Stale(Some(_))));
        let lock = acquire(tmp.path(), &this_process("now", &test_ctx())).unwrap();
        assert!(lock.is_ok(), "the reused pid's lock is broken and taken");
        assert_eq!(read_info(tmp.path()).unwrap().pid_start, Some(mine));
    }
}
