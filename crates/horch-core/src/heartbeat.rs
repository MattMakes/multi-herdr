//! The 1 liveness scheme of horch's background jobs: the judge job (CMP-16)
//! and the compaction job (CTX-15).
//!
//! The job rewrites a [`Heartbeat`] file in its job dir every
//! [`HEARTBEAT_EVERY`] ([`start`]). A reader asks [`liveness`] whether the
//! job still runs: a fresh heartbeat whose pid is still the job, or a spawn
//! so recent that the first beat may not be written yet. Each job keeps its
//! own result files and its own state type on top of this.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::fsx;
use crate::procid;

/// The heartbeat file in a job dir.
pub const HEARTBEAT_FILE: &str = "heartbeat";

/// How often the job rewrites its heartbeat.
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(2);

/// A heartbeat older than this, or a job that never wrote one this long
/// after its spawn, is lost.
pub const STALE_AFTER: Duration = Duration::from_secs(30);

/// `heartbeat`: the job is alive at `at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub pid: u32,
    pub at: String,
    /// The job's [`procid::start_time`]. With it, a pid that ended and was
    /// given to another program is not the job: not alive, not killed.
    /// `None` in a heartbeat written before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<u64>,
}

/// The heartbeat in `dir`, and whether its pid runs with its start time
/// ([`procid::alive`]). Never writes.
pub fn read(dir: &Path) -> (Option<Heartbeat>, bool) {
    let hb: Option<Heartbeat> = std::fs::read(dir.join(HEARTBEAT_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let alive = hb.as_ref().is_some_and(|h| procid::alive(h.pid, h.started));
    (hb, alive)
}

/// What the heartbeat says about a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Nothing was spawned.
    NotStarted,
    /// The job is alive. `pid` is 0 until the first heartbeat.
    Running { pid: u32 },
    /// No live, fresh heartbeat.
    Lost,
}

/// Pure: heartbeat fresh and pid alive → running; a heartbeat otherwise →
/// lost; no heartbeat and spawned less than `stale_after` ago → running;
/// not spawned → not started; else lost.
pub fn liveness(
    hb: Option<&Heartbeat>,
    pid_alive: bool,
    spawned_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    stale_after: Duration,
) -> Liveness {
    let stale = chrono::Duration::from_std(stale_after).unwrap_or(chrono::Duration::MAX);
    let fresh = |at: DateTime<Utc>| now - at <= stale;
    if let Some(hb) = hb {
        let at = crate::clock::parse(&hb.at);
        return if pid_alive && at.is_some_and(fresh) {
            Liveness::Running { pid: hb.pid }
        } else {
            Liveness::Lost
        };
    }
    match spawned_at {
        None => Liveness::NotStarted,
        Some(at) if fresh(at) => Liveness::Running { pid: 0 },
        Some(_) => Liveness::Lost,
    }
}

/// The running heartbeat writer of [`start`]. Dropping it stops the writer.
#[derive(Debug)]
pub struct Beat {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Beat {
    /// Stop the writer and wait for its thread.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Beat {
    fn drop(&mut self) {
        self.halt();
    }
}

/// Write `heartbeat` in `dir` once now, then rewrite it every
/// [`HEARTBEAT_EVERY`] until the [`Beat`] is stopped or dropped. The first
/// beat is written before this returns: a job that ends before the thread
/// first runs still leaves a heartbeat, and the judge coordinator emits
/// `judge.started` from it.
pub fn start(dir: &Path) -> Beat {
    let dir: PathBuf = dir.to_path_buf();
    let pid = std::process::id();
    beat(&dir, pid);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::spawn(move || {
        let tick = Duration::from_millis(100);
        let mut next = Instant::now() + HEARTBEAT_EVERY;
        while !flag.load(Ordering::SeqCst) {
            if Instant::now() >= next {
                beat(&dir, pid);
                next = Instant::now() + HEARTBEAT_EVERY;
            }
            std::thread::sleep(tick);
        }
    });
    Beat {
        stop,
        thread: Some(thread),
    }
}

/// Write one heartbeat for `pid` in `dir`.
fn beat(dir: &Path, pid: u32) {
    let hb = Heartbeat {
        pid,
        // A reader kills this job only while its pid has this start time:
        // never a later program that was given the pid.
        started: procid::start_time(pid),
        // Wall-clock, never a pinned HORCH_NOW: the reader compares it with
        // real file times.
        at: crate::clock::stamp(Utc::now()),
    };
    if let Ok(bytes) = serde_json::to_vec(&hb) {
        let _ = fsx::write_atomic(&dir.join(HEARTBEAT_FILE), &bytes, fsx::PRIVATE_FILE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_shape() {
        let old: Heartbeat = serde_json::from_str(r#"{"pid":7,"at":"x"}"#).unwrap();
        assert_eq!(old.started, None, "a heartbeat from before start times");
        let hb = Heartbeat {
            pid: 7,
            at: "x".into(),
            started: Some(42),
        };
        assert_eq!(
            serde_json::to_string(&hb).unwrap(),
            r#"{"pid":7,"at":"x","started":42}"#
        );
    }

    /// The first heartbeat exists when `start` returns, even when the job
    /// stops at once and the thread never beats.
    #[test]
    fn heartbeat_is_written_before_start_returns() {
        let tmp = tempfile::tempdir().unwrap();
        let beat = start(tmp.path());
        let written = tmp.path().join(HEARTBEAT_FILE);
        assert!(written.is_file(), "no heartbeat before the job runs");
        beat.stop();
        let (hb, alive) = read(tmp.path());
        let hb = hb.unwrap();
        assert_eq!(hb.pid, std::process::id());
        assert_eq!(hb.started, procid::start_time(hb.pid));
        assert!(alive, "this test's own process runs");
    }
}
