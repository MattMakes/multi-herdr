use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::fsx;
use crate::procid;

pub const HEARTBEAT_FILE: &str = "heartbeat";

pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(2);

pub const STALE_AFTER: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub pid: u32,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<u64>,
}

pub fn read(dir: &Path) -> (Option<Heartbeat>, bool) {
    let hb: Option<Heartbeat> = std::fs::read(dir.join(HEARTBEAT_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let alive = hb.as_ref().is_some_and(|h| procid::alive(h.pid, h.started));
    (hb, alive)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    NotStarted,
    Running { pid: u32 },
    Lost,
}

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

#[derive(Debug)]
pub struct Beat {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Beat {
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

fn beat(dir: &Path, pid: u32) {
    let hb = Heartbeat {
        pid,
        started: procid::start_time(pid),
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
