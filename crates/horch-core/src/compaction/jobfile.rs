//! The compaction job dir, `<state_root>/compact/<workspace>-<role>/`
//! (design §6.8, §6.8a). The same layout idea as the judge job dir, on the
//! shared [`crate::heartbeat`]:
//!
//! | file | written by | content |
//! |---|---|---|
//! | `job.log` | the parent creates it before the spawn; the child appends | 1 line per step |
//! | `spawned` | the parent, right after the spawn | [`Spawned`] |
//! | `job.json` | the child, `create_new`, then rewritten at each step | [`JobRecord`] |
//! | `heartbeat` | the child, through [`crate::heartbeat::start`] | [`crate::heartbeat::Heartbeat`] |
//! | `lost` | the first reader that finds the job lost, `create_new` | `{"at": "<UTC>"}` |
//!
//! The child removes `job.json`, `heartbeat` and `spawned` on every exit;
//! `job.log` stays.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::policy::JobLiveness;
use crate::fsx;
use crate::heartbeat::{self, Liveness, HEARTBEAT_FILE, STALE_AFTER};

pub const JOB_FILE: &str = "job.json";
pub const SPAWNED_FILE: &str = "spawned";
pub const LOST_FILE: &str = "lost";
pub const LOG_FILE: &str = "job.log";

/// The `job.json` format.
pub const JOB_VERSION: u32 = 1;

/// How long [`start_locked`] waits for the start lock.
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// `job.json`: which job runs, and its step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRecord {
    pub v: u32,
    pub role: String,
    pub record_id: String,
    pub started_at: String,
    /// The kebab-case [`super::job::Step`] name.
    pub step: String,
}

impl JobRecord {
    /// The record for a `job.json` that is absent or does not parse.
    fn unknown() -> JobRecord {
        JobRecord {
            v: JOB_VERSION,
            role: String::new(),
            record_id: String::new(),
            started_at: String::new(),
            step: "unknown".into(),
        }
    }
}

/// `spawned`: the parent started the child `pid` at `at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spawned {
    pub pid: u32,
    pub at: String,
}

/// What [`start_locked`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    Spawned {
        pid: u32,
    },
    /// A live job holds the dir; nothing was spawned.
    AlreadyRunning,
}

/// `<state_root>/compact`.
pub fn compact_root(state_root: &Path) -> PathBuf {
    state_root.join("compact")
}

/// The job dir of `role` in `workspace`. Characters outside
/// `[A-Za-z0-9._-]` become `-`.
pub fn job_dir(state_root: &Path, workspace: &str, role: &str) -> PathBuf {
    let safe = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '-'
                }
            })
            .collect()
    };
    compact_root(state_root).join(format!("{}-{}", safe(workspace), safe(role)))
}

/// `job.json` in `dir`: `None` when it is absent, `Some(None)` when it does
/// not parse.
fn read_job(dir: &Path) -> Option<Option<JobRecord>> {
    let bytes = std::fs::read(dir.join(JOB_FILE)).ok()?;
    Some(serde_json::from_slice(&bytes).ok())
}

/// When the job was spawned: the `spawned` file, else the `job.json` file
/// time (the child can write it before the parent writes `spawned`).
fn spawned_at(dir: &Path) -> Option<DateTime<Utc>> {
    let file = std::fs::read(dir.join(SPAWNED_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice::<Spawned>(&b).ok())
        .and_then(|s| crate::clock::parse(&s.at));
    file.or_else(|| {
        [SPAWNED_FILE, JOB_FILE].iter().find_map(|name| {
            std::fs::metadata(dir.join(name))
                .and_then(|m| m.modified())
                .ok()
                .map(DateTime::<Utc>::from)
        })
    })
}

/// None: no `job.json` and no starting job. Live: [`heartbeat::liveness`]
/// is Running. Lost: `job.json` exists and liveness is Lost (dead pid, or a
/// stale heartbeat), with the step `job.json` names. `now` is the wall
/// clock: heartbeats and spawn times are real times.
pub fn liveness(dir: &Path, now: DateTime<Utc>) -> JobLiveness {
    let (hb, alive) = heartbeat::read(dir);
    let job = read_job(dir);
    let live = heartbeat::liveness(hb.as_ref(), alive, spawned_at(dir), now, STALE_AFTER);
    match (live, job) {
        (Liveness::Running { .. }, _) => JobLiveness::Live,
        (Liveness::Lost, Some(job)) => JobLiveness::Lost {
            step: job.map_or_else(|| "unknown".into(), |j| j.step),
        },
        _ => JobLiveness::None,
    }
}

/// Create `lost` with `create_new`. `Some(job.json content)` only for the 1
/// caller that created it: that caller records the `compact-failed` event.
/// A `job.json` that does not parse gives a record with step `unknown`.
pub fn claim_lost(dir: &Path) -> Result<Option<JobRecord>> {
    let path = dir.join(LOST_FILE);
    let body = serde_json::json!({ "at": crate::clock::now_stamp() }).to_string();
    let claimed = match create_new(&path, body.as_bytes()) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => Err(e).context(format!("creating {}", path.display()))?,
    };
    Ok(claimed.then(|| read_job(dir).flatten().unwrap_or_else(JobRecord::unknown)))
}

/// Create `job.json` with `create_new`, so 2 jobs for 1 role cannot both
/// run. The second gets `ErrorKind::AlreadyExists`.
pub fn create_job(dir: &Path, job: &JobRecord) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(job).map_err(std::io::Error::other)?;
    create_new(&dir.join(JOB_FILE), &bytes)
}

/// Rewrite `job.json` atomically with the job's new step.
pub fn write_step(dir: &Path, job: &JobRecord) -> Result<()> {
    let bytes = serde_json::to_vec(job)?;
    fsx::write_atomic(&dir.join(JOB_FILE), &bytes, fsx::PRIVATE_FILE)?;
    Ok(())
}

/// The child's exit: remove `job.json`, `heartbeat` and `spawned`. `job.log`
/// stays.
pub fn clear_on_exit(dir: &Path) {
    remove(dir, &[JOB_FILE, HEARTBEAT_FILE, SPAWNED_FILE]);
}

/// Under a `fsx::DirLock` in `<state_root>/compact/`: check liveness again;
/// when Lost or None, remove `job.json`, `heartbeat`, `spawned` and `lost`
/// (keep `job.log`), run `spawn`, write `spawned`. Live: return
/// [`Started::AlreadyRunning`] and spawn nothing. So 2 callers cannot
/// remove each other's new job.
pub fn start_locked(dir: &Path, spawn: impl FnOnce() -> Result<u32>) -> Result<Started> {
    let _lock = lock_job_dir(dir)?;
    // Wall clock: heartbeats and spawn times are real times.
    if liveness(dir, Utc::now()) == JobLiveness::Live {
        return Ok(Started::AlreadyRunning);
    }
    fsx::ensure_private_dir(dir)?;
    remove(dir, &[JOB_FILE, HEARTBEAT_FILE, SPAWNED_FILE, LOST_FILE]);
    record_spawn(dir, spawn()?)
}

/// The start lock of `dir`, in the job dir's parent under the job dir's name.
fn lock_job_dir(dir: &Path) -> Result<fsx::DirLockGuard> {
    let root = dir.parent().context("the job dir has no parent")?;
    let name = dir
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .context("the job dir has no name")?;
    fsx::ensure_private_dir(root)?;
    Ok(fsx::DirLock::acquire(
        root,
        name,
        STALE_AFTER,
        LOCK_TIMEOUT,
    )?)
}

/// Write `spawned`: the parent started the child `pid` now.
fn record_spawn(dir: &Path, pid: u32) -> Result<Started> {
    let spawned = Spawned {
        pid,
        at: crate::clock::stamp(Utc::now()),
    };
    fsx::write_atomic(
        &dir.join(SPAWNED_FILE),
        &serde_json::to_vec(&spawned)?,
        fsx::PRIVATE_FILE,
    )?;
    Ok(Started::Spawned { pid })
}

fn remove(dir: &Path, names: &[&str]) {
    for name in names {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

fn create_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, fsx::PRIVATE_FILE);
    let mut file = opts.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(step: &str) -> JobRecord {
        JobRecord {
            v: JOB_VERSION,
            role: "w".into(),
            record_id: "r1".into(),
            started_at: "2026-10-07T00:00:00Z".into(),
            step: step.into(),
        }
    }

    fn job_dir_in(root: &Path) -> PathBuf {
        let dir = job_dir(root, "ws", "role");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn claim_lost_returns_the_record_once() {
        let root = tempfile::tempdir().unwrap();
        let dir = job_dir_in(root.path());
        create_job(&dir, &record("send")).unwrap();
        assert_eq!(claim_lost(&dir).unwrap(), Some(record("send")));
        assert!(dir.join(LOST_FILE).exists());
        assert_eq!(claim_lost(&dir).unwrap(), None);
    }

    #[test]
    fn claim_lost_names_an_unparsable_job_unknown() {
        let root = tempfile::tempdir().unwrap();
        let dir = job_dir_in(root.path());
        std::fs::write(dir.join(JOB_FILE), b"not json").unwrap();
        let job = claim_lost(&dir).unwrap().unwrap();
        assert_eq!(job.step, "unknown");
        assert_eq!(job.v, JOB_VERSION);
        assert_eq!(job.role, "");
    }

    #[test]
    fn claim_lost_without_a_job_file_names_the_step_unknown() {
        let root = tempfile::tempdir().unwrap();
        let dir = job_dir_in(root.path());
        assert_eq!(claim_lost(&dir).unwrap().unwrap().step, "unknown");
    }

    #[test]
    fn claim_lost_errors_when_the_dir_is_missing() {
        let root = tempfile::tempdir().unwrap();
        let dir = job_dir(root.path(), "ws", "missing");
        let err = claim_lost(&dir).unwrap_err();
        assert!(format!("{err:#}").contains("creating"), "{err:#}");
    }

    #[test]
    fn start_locked_spawns_and_records_the_pid() {
        let root = tempfile::tempdir().unwrap();
        let dir = job_dir(root.path(), "ws", "role");
        let started = start_locked(&dir, || Ok(4242)).unwrap();
        assert_eq!(started, Started::Spawned { pid: 4242 });
        let spawned: Spawned =
            serde_json::from_slice(&std::fs::read(dir.join(SPAWNED_FILE)).unwrap()).unwrap();
        assert_eq!(spawned.pid, 4242);
    }

    #[test]
    fn start_locked_clears_old_files_before_the_spawn() {
        let root = tempfile::tempdir().unwrap();
        let dir = job_dir_in(root.path());
        for name in [JOB_FILE, HEARTBEAT_FILE, LOST_FILE, LOG_FILE] {
            let path = dir.join(name);
            std::fs::write(&path, b"old").unwrap();
            // Old enough that the job counts as lost, not starting.
            let old = std::time::SystemTime::now() - Duration::from_secs(3600);
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        start_locked(&dir, || Ok(7)).unwrap();
        for name in [JOB_FILE, HEARTBEAT_FILE, LOST_FILE] {
            assert!(!dir.join(name).exists(), "{name} stays");
        }
        assert!(dir.join(LOG_FILE).exists());
    }

    #[test]
    fn start_locked_is_already_running_while_a_job_lives() {
        let root = tempfile::tempdir().unwrap();
        let dir = job_dir(root.path(), "ws", "role");
        start_locked(&dir, || Ok(1)).unwrap();
        let second = start_locked(&dir, || panic!("a live job must not spawn"));
        assert_eq!(second.unwrap(), Started::AlreadyRunning);
    }

    #[test]
    fn start_locked_returns_a_spawn_error_and_writes_no_spawned_file() {
        let root = tempfile::tempdir().unwrap();
        let dir = job_dir(root.path(), "ws", "role");
        let err = start_locked(&dir, || anyhow::bail!("no fork")).unwrap_err();
        assert!(format!("{err:#}").contains("no fork"));
        assert!(!dir.join(SPAWNED_FILE).exists());
    }
}
