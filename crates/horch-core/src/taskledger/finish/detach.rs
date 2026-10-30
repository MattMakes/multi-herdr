//! A finish that runs detached (#160 phase 2). `horch ledger finish` starts
//! `horch ledger finish --run-steps` in its own process group and returns at
//! once; the steps' result goes to the worker as durable mail. The run file
//! names the detached process by its PID and start time, so a PID that the
//! OS gave to another process is not taken for it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The kind of the worker wait that covers a detached finish.
pub const FINISH_WAIT: &str = "finish";

/// How long a detached run waits for its run file to name it.
const RECORD_WAIT: Duration = Duration::from_secs(10);

/// The detached process of 1 attempt's finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFile {
    pub pid: u32,
    /// The process start time, from [`crate::procid::start_time`].
    pub started: u64,
}

/// What the run file of an attempt says now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detached {
    /// No detached finish runs.
    Idle,
    /// The detached finish runs.
    Running(RunFile),
    /// The recorded process is gone, and it left no result.
    Lost(RunFile),
}

/// The run file of the attempt of `worker_id` at `task_id`.
pub fn run_path(evidence_root: &Path, task_id: &str, worker_id: &str) -> PathBuf {
    super::task_dir(evidence_root, task_id)
        .join(worker_id)
        .join("finish-run.json")
}

/// The log of the detached run, beside its run file.
pub fn log_path(run: &Path) -> PathBuf {
    run.with_file_name("finish-run.log")
}

fn read(path: &Path) -> Option<RunFile> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The state of the detached finish recorded at `path`.
pub fn state(path: &Path) -> Detached {
    match read(path) {
        None => Detached::Idle,
        Some(r) if crate::procid::is_same(r.pid, r.started) => Detached::Running(r),
        Some(r) => Detached::Lost(r),
    }
}

/// Record process `pid` as the detached finish. Fails when its start time
/// cannot be read: then it cannot be told apart from a later process.
pub fn record(path: &Path, pid: u32) -> Result<RunFile> {
    let started = crate::procid::start_time(pid)
        .with_context(|| format!("reading the start time of the detached finish (PID {pid})"))?;
    let run = RunFile { pid, started };
    std::fs::create_dir_all(path.parent().expect("a worker folder"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&run)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(run)
}

/// In the detached process: wait until the run file names this process, so
/// it never ends before its starter recorded it. Whether it does.
pub fn wait_for_record(path: &Path) -> bool {
    let me = std::process::id();
    let until = Instant::now() + RECORD_WAIT;
    while read(path).map(|r| r.pid) != Some(me) {
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Forget the detached finish: it ended and its result is sent.
pub fn clear(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_file_tells_a_live_run_from_a_lost_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = run_path(dir.path(), "task", "worker");
        assert_eq!(state(&path), Detached::Idle);
        let me = record(&path, std::process::id()).unwrap();
        assert_eq!(state(&path), Detached::Running(me));
        assert!(wait_for_record(&path), "the file names this process");
        // The same PID with another start time is another process.
        let reused = RunFile {
            started: me.started + 1,
            ..me
        };
        std::fs::write(&path, serde_json::to_vec(&reused).unwrap()).unwrap();
        assert_eq!(state(&path), Detached::Lost(reused));
        clear(&path);
        assert_eq!(state(&path), Detached::Idle);
    }
}
