//! The judge job: the only detached process in the dataset mode (CMP-16).
//!
//! [`schedule`] starts `multi-herdr-dataset judge-job` in a new session, so
//! the job outlives the coordinator. The job writes only its job dir,
//! `jobs/<round>/judge-<n>/`:
//!
//! | File | Written by | Content |
//! |---|---|---|
//! | `job.log` | the coordinator, before the spawn | the job's stdout and stderr |
//! | `heartbeat` | the job, every [`HEARTBEAT_EVERY`] | [`Heartbeat`] |
//! | `output.json` | the job, once, ≤ [`OUTPUT_CAP_BYTES`] | the judge's answer text |
//! | `exit.json` | the job, once, last | [`JobExit`] |
//!
//! [`discover`] reads those files and says what the job is doing. The
//! decision itself is the pure [`decide`], so a table test covers it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::fsx;
use crate::ids::{RoundId, SessionId};
use crate::runtime::RuntimeContext;

pub const HEARTBEAT_FILE: &str = "heartbeat";
pub const OUTPUT_FILE: &str = "output.json";
pub const EXIT_FILE: &str = "exit.json";
pub const LOG_FILE: &str = "job.log";

/// The most a judge answer may hold (design §4.7, SEC-06).
pub const OUTPUT_CAP_BYTES: usize = 1024 * 1024;

/// How often the job rewrites its heartbeat.
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(2);

/// A heartbeat older than this, or a job that never wrote one this long
/// after its spawn, is lost.
pub const DEFAULT_STALE_AFTER: Duration = Duration::from_secs(30);

/// The dataset binary, a sibling of `horch`.
pub const DATASET_BIN: &str = "multi-herdr-dataset";

/// One judge attempt to start.
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeJobSpec {
    pub round: RoundId,
    pub attempt: u32,
    /// The sealed bundle; the judge's working directory.
    pub input_dir: PathBuf,
    pub job_dir: PathBuf,
    /// Minted by the coordinator and kept on the execution record.
    pub session: SessionId,
    pub model: String,
    pub effort: String,
    pub timeout: Duration,
}

/// `heartbeat`: the job is alive at `at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub pid: u32,
    pub at: String,
}

/// `exit.json`: how the job ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobExit {
    pub reason: ExitReason,
    /// The judge CLI's exit code, when it exited on its own.
    pub code: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    /// `output.json` was written.
    Ok,
    /// The CLI failed, or the job could not run it.
    Crash,
    /// The CLI ran past the timeout and was killed.
    Timeout,
    /// The answer was over [`OUTPUT_CAP_BYTES`]; nothing was written.
    TooLarge,
}

/// What a job dir says about its job.
///
/// The design's `Exited { code }` became [`JobState::Exited`] with the whole
/// [`JobExit`], because the coordinator records a crash, a timeout and an
/// oversized answer differently. `NotStarted` covers a coordinator that
/// stopped between `judge.scheduled` and the spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    /// No `job.log`: nothing was spawned for this attempt.
    NotStarted,
    /// The job has not finished and is alive. `pid` is 0 until the first
    /// heartbeat.
    Running { pid: u32 },
    /// `output.json` exists. It is parsed whatever `exit.json` says.
    OutputPresent(PathBuf),
    /// The job ended without an answer.
    Exited(JobExit),
    /// No answer, no exit file, and no live, fresh heartbeat.
    Lost,
}

/// The file facts [`decide`] reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobFacts {
    pub output: bool,
    /// `Some(None)`: `exit.json` exists but does not parse.
    pub exit: Option<Option<JobExit>>,
    pub heartbeat: Option<Heartbeat>,
    /// When the coordinator created `job.log` (the spawn), if it did.
    pub spawned_at: Option<DateTime<Utc>>,
    /// Whether the heartbeat's pid is alive.
    pub pid_alive: bool,
}

/// Pure: output present → parse; exit file → exited; heartbeat fresh and
/// pid alive → running; spawned less than `stale_after` ago with no
/// heartbeat yet → running; not spawned → not started; else lost.
pub fn decide(facts: &JobFacts, now: DateTime<Utc>, stale_after: Duration) -> JobState {
    let stale = chrono::Duration::from_std(stale_after).unwrap_or(chrono::Duration::MAX);
    let fresh = |at: DateTime<Utc>| now - at <= stale;
    if facts.output {
        return JobState::OutputPresent(PathBuf::from(OUTPUT_FILE));
    }
    if let Some(exit) = &facts.exit {
        return JobState::Exited(exit.clone().unwrap_or(JobExit {
            reason: ExitReason::Crash,
            code: None,
        }));
    }
    if let Some(hb) = &facts.heartbeat {
        let at = crate::clock::parse(&hb.at);
        return if facts.pid_alive && at.is_some_and(fresh) {
            JobState::Running { pid: hb.pid }
        } else {
            JobState::Lost
        };
    }
    match facts.spawned_at {
        None => JobState::NotStarted,
        Some(at) if fresh(at) => JobState::Running { pid: 0 },
        Some(_) => JobState::Lost,
    }
}

/// Read the facts of `job_dir`. Never writes.
pub fn job_facts(job_dir: &Path) -> JobFacts {
    let heartbeat: Option<Heartbeat> = std::fs::read(job_dir.join(HEARTBEAT_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let exit = std::fs::read(job_dir.join(EXIT_FILE))
        .ok()
        .map(|b| serde_json::from_slice::<JobExit>(&b).ok());
    let spawned_at = std::fs::metadata(job_dir.join(LOG_FILE))
        .and_then(|m| m.created().or_else(|_| m.modified()))
        .ok()
        .map(DateTime::<Utc>::from);
    JobFacts {
        output: job_dir.join(OUTPUT_FILE).is_file(),
        exit,
        pid_alive: heartbeat.as_ref().is_some_and(|h| fsx::pid_alive(h.pid)),
        heartbeat,
        spawned_at,
    }
}

/// What the job in `job_dir` is doing at `now`. `OutputPresent` carries the
/// full path.
pub fn discover(job_dir: &Path, now: DateTime<Utc>, stale_after: Duration) -> JobState {
    match decide(&job_facts(job_dir), now, stale_after) {
        JobState::OutputPresent(_) => JobState::OutputPresent(job_dir.join(OUTPUT_FILE)),
        other => other,
    }
}

/// The dataset binary: this process when it is the dataset binary, else its
/// sibling next to the current executable.
pub fn dataset_exe(ctx: &RuntimeContext) -> Result<PathBuf> {
    let exe = ctx.bins.exe()?;
    if exe.file_stem().and_then(|s| s.to_str()) == Some(DATASET_BIN) {
        return Ok(exe);
    }
    let sibling = exe
        .parent()
        .context("the current executable has no directory")?
        .join(format!("{DATASET_BIN}{}", std::env::consts::EXE_SUFFIX));
    if !sibling.is_file() {
        bail!("{} is not installed next to {}", DATASET_BIN, exe.display());
    }
    Ok(sibling)
}

/// The `judge-job` argv after the program name.
pub fn job_args(spec: &JudgeJobSpec) -> Vec<String> {
    vec![
        "judge-job".into(),
        "--round".into(),
        spec.round.to_string(),
        "--attempt".into(),
        spec.attempt.to_string(),
        "--input-dir".into(),
        spec.input_dir.to_string_lossy().into_owned(),
        "--session-id".into(),
        spec.session.to_string(),
        "--model".into(),
        spec.model.clone(),
        "--effort".into(),
        spec.effort.clone(),
        "--timeout-s".into(),
        spec.timeout.as_secs().max(1).to_string(),
    ]
}

/// Start the judge job for `spec`, detached (a new session), and return its
/// pid. Creates the job dir and `job.log`; the job writes the rest. The child
/// is reaped by a thread, so a finished job never lingers as a zombie that
/// [`fsx::pid_alive`] would count as alive.
pub fn schedule(ctx: &RuntimeContext, spec: &JudgeJobSpec) -> Result<u32> {
    let exe = dataset_exe(ctx)?;
    if let Some(parent) = spec.job_dir.parent() {
        fsx::ensure_private_dir(parent)?;
    }
    fsx::ensure_private_dir(&spec.job_dir)?;
    let log_path = spec.job_dir.join(LOG_FILE);
    let log = open_log(&log_path)?;
    let mut cmd = Command::new(&exe);
    cmd.args(job_args(spec))
        .current_dir(&spec.job_dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone().context("duplicating the job log")?)
        .stderr(log)
        .env("HORCH_STATE_DIR", &ctx.paths.state_root)
        .env("HORCH_PROJECT_DIR", ctx.paths.project()?);
    crate::runtime::process::strip_forbidden(&mut cmd);
    let mut child = crate::runtime::process::spawn_detached(&mut cmd)
        .with_context(|| format!("starting {}", exe.display()))?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

fn open_log(path: &Path) -> Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, fsx::PRIVATE_FILE);
    opts.open(path)
        .with_context(|| format!("opening {}", path.display()))
}

/// Kill a lost job and everything it started. The job leads its own session
/// and process group, so the group id is its pid.
pub fn kill_job(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(unix)]
    unsafe {
        // SAFETY: plain syscalls on a pid we started; failure is ignored.
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        crate::clock::parse(s).unwrap()
    }

    #[test]
    fn job_args_name_every_field() {
        let spec = JudgeJobSpec {
            round: "r1".parse().unwrap(),
            attempt: 2,
            input_dir: "/b".into(),
            job_dir: "/j".into(),
            session: SessionId::new("s1").unwrap(),
            model: "opus".into(),
            effort: "high".into(),
            timeout: Duration::from_millis(1500),
        };
        assert_eq!(
            job_args(&spec).join(" "),
            "judge-job --round r1 --attempt 2 --input-dir /b --session-id s1 \
             --model opus --effort high --timeout-s 1"
        );
    }

    #[test]
    fn exit_file_shape() {
        let e = JobExit {
            reason: ExitReason::TooLarge,
            code: Some(0),
        };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"reason":"too_large","code":0}"#
        );
        let now = at("2026-10-02T12:00:00Z");
        let facts = JobFacts {
            exit: Some(Some(e.clone())),
            ..JobFacts::default()
        };
        assert_eq!(
            decide(&facts, now, DEFAULT_STALE_AFTER),
            JobState::Exited(e)
        );
    }
}
