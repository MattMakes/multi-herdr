use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const FINISH_WAIT: &str = "background";

const RECORD_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFile {
    pub pid: u32,
    pub started: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detached {
    Idle,
    Running(RunFile),
    Lost(RunFile),
}

pub fn run_path(evidence_root: &Path, task_id: &str, worker_id: &str) -> PathBuf {
    super::task_dir(evidence_root, task_id)
        .join(worker_id)
        .join("finish-run.json")
}

pub fn log_path(run: &Path) -> PathBuf {
    run.with_file_name("finish-run.log")
}

fn read(path: &Path) -> Option<RunFile> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn state(path: &Path) -> Detached {
    match read(path) {
        None => Detached::Idle,
        Some(r) if crate::procid::is_same(r.pid, r.started) => Detached::Running(r),
        Some(r) => Detached::Lost(r),
    }
}

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
