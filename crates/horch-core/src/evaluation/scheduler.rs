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
use crate::procid;
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
pub(crate) const DEFAULT_STALE_AFTER: Duration = Duration::from_secs(30);

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
    /// The job's [`procid::start_time`]. With it, a pid that ended and was
    /// given to another program is not the job: not alive, not killed.
    /// `None` in a heartbeat written before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<u64>,
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
    /// Whether the heartbeat's pid is alive and, when the heartbeat has a
    /// start time, is still the job.
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
pub(crate) fn job_facts(job_dir: &Path) -> JobFacts {
    job_facts_with(job_dir, || {})
}

/// [`job_facts`], with `between` run after the liveness look and before the
/// result files are read.
///
/// The order matters. The job writes `output.json`, then `exit.json`, then
/// exits. Liveness is looked at first: a job that was alive then and has
/// ended since left its files before it ended, so the file reads see them.
/// Files read first could miss an answer that a job wrote and then exited
/// right after, and its dead pid would read as Lost: a valid answer thrown
/// away and the judge run again.
fn job_facts_with(job_dir: &Path, between: impl FnOnce()) -> JobFacts {
    let heartbeat: Option<Heartbeat> = std::fs::read(job_dir.join(HEARTBEAT_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let pid_alive = heartbeat
        .as_ref()
        .is_some_and(|h| procid::alive(h.pid, h.started));
    between();
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
        pid_alive,
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
pub(crate) fn dataset_exe(ctx: &RuntimeContext) -> Result<PathBuf> {
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

/// Kill the job of `hb` and everything it started, and say whether a
/// signal was sent. The job leads its own session and process group, so the
/// group id is its pid.
///
/// Only while the pid is still the job: same pid, same start time. The pid
/// comes from a file, and a job that has ended frees it for any other
/// program. A heartbeat without a start time names no process for certain,
/// so it is not killed either.
///
/// The group is signalled first, while the leader is checked to run: a
/// running leader keeps its group id from being given out. The window is
/// from the check to `killpg`: the leader would have to end, its group
/// empty, and the id go to a new group between 2 calls. The leader itself
/// gets [`procid::signal_same`], which closes its own window on Linux.
pub(crate) fn kill_job(hb: &Heartbeat) -> bool {
    let Some(started) = hb.started else {
        return false;
    };
    if !procid::is_same(hb.pid, started) {
        return false;
    }
    #[cfg(unix)]
    {
        if let Some(group) = procid::os_pid(hb.pid) {
            // SAFETY: plain syscall on the job's group, checked just above;
            // failure is ignored.
            unsafe { libc::killpg(group, libc::SIGKILL) };
        }
        procid::signal_same(hb.pid, started, libc::SIGKILL);
    }
    cfg!(unix)
}

/// What [`kill_lost_job`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LostKill {
    /// The job still ran as itself, and [`kill_job`] killed it and its group.
    pub job: bool,
    /// Members of the dead job's group that were killed: its orphans.
    pub killed: Vec<u32>,
    /// Members that started after the last heartbeat: not the job's.
    pub kept: Vec<u32>,
}

/// Kill what is left of the lost job in `job_dir`.
///
/// A job that still runs as itself is killed with [`kill_job`]. A job whose
/// pid no process has can leave its judge CLI running in its group. Each
/// member of that group that started at or before the heartbeat file's
/// last write is killed: it ran while the job was alive, so it is the job's.
/// The id is not the job's for certain: once the job's group empties, a
/// later program can get the pid, lead a group, and die. But that program
/// started after the job died, so its members did too, and are kept. Each
/// member's start time is checked again just before its signal. A pid that
/// another program has now is never signalled. Windows lists no members:
/// only [`kill_job`] applies there.
pub(crate) fn kill_lost_job(job_dir: &Path) -> LostKill {
    let path = job_dir.join(HEARTBEAT_FILE);
    let Some(hb) = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Heartbeat>(&b).ok())
    else {
        return LostKill::default();
    };
    let mut done = LostKill {
        job: kill_job(&hb),
        ..LostKill::default()
    };
    let last_beat = std::fs::metadata(&path).and_then(|m| m.modified());
    if done.job || fsx::pid_alive(hb.pid) {
        return done;
    }
    let Ok(last_beat) = last_beat else {
        return done;
    };
    for pid in procid::group_members(hb.pid) {
        let ours = procid::started_by(pid, last_beat);
        match procid::start_time(pid) {
            Some(started) if ours && kill_member(pid, hb.pid, started) => done.killed.push(pid),
            _ => done.kept.push(pid),
        }
    }
    done
}

/// SIGKILL `pid` while it is still in group `pgid` with start time
/// `started`, and say whether a signal was sent.
fn kill_member(pid: u32, pgid: u32, started: u64) -> bool {
    #[cfg(unix)]
    {
        let (Some(os), Some(group)) = (procid::os_pid(pid), procid::os_pid(pgid)) else {
            return false;
        };
        // SAFETY: plain syscall; a pid that has gone answers -1.
        if unsafe { libc::getpgid(os) } != group {
            return false;
        }
        procid::signal_same(pid, started, libc::SIGKILL)
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, pgid, started);
        false
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

    /// A job record is killed only while its pid is still the job. The
    /// processes here are this test's own children; a record that does not
    /// match is never signalled.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn kill_job_only_kills_the_same_process() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let started = procid::start_time(pid).unwrap();
        let hb = |started| Heartbeat {
            pid,
            at: "x".into(),
            started,
        };
        assert!(
            !kill_job(&hb(None)),
            "no start time: not the job for certain"
        );
        assert!(!kill_job(&hb(Some(started + 1))), "a reused pid");
        assert_eq!(child.try_wait().unwrap(), None, "the child still runs");
        assert!(kill_job(&hb(Some(started))));
        assert!(!child.wait().unwrap().success(), "killed");
        // Ended and reaped: its pid is free, and the record names nothing.
        assert!(!kill_job(&hb(Some(started))));
        assert!(!kill_job(&Heartbeat {
            pid: 0,
            at: "x".into(),
            started: Some(0),
        }));
    }

    /// F4: the job answers and exits while the coordinator looks. The hook
    /// puts the whole end of the job (output, exit file, exit) between the
    /// liveness look and the file reads. The answer is found; the job is
    /// not Lost.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_job_that_answers_and_exits_mid_look_is_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let hb = Heartbeat {
            pid,
            at: crate::clock::stamp(Utc::now()),
            started: procid::start_time(pid),
        };
        std::fs::write(
            dir.path().join(HEARTBEAT_FILE),
            serde_json::to_vec(&hb).unwrap(),
        )
        .unwrap();
        let facts = job_facts_with(dir.path(), || {
            std::fs::write(dir.path().join(OUTPUT_FILE), b"{}").unwrap();
            let exit = JobExit {
                reason: ExitReason::Ok,
                code: Some(0),
            };
            std::fs::write(
                dir.path().join(EXIT_FILE),
                serde_json::to_vec(&exit).unwrap(),
            )
            .unwrap();
            child.kill().unwrap();
            child.wait().unwrap();
        });
        assert!(facts.output, "{facts:?}");
        assert!(matches!(
            decide(&facts, Utc::now(), DEFAULT_STALE_AFTER),
            JobState::OutputPresent(_)
        ));
        // The same job seen after it ended: still the answer, never Lost.
        assert!(matches!(
            discover(dir.path(), Utc::now(), DEFAULT_STALE_AFTER),
            JobState::OutputPresent(_)
        ));
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
