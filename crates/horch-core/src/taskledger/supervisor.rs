//! What `horch worker` (the supervisor, the wrapper of an attempt) reports
//! about its agent (section 4.6, #34). From Phase 5 it writes no ledger row
//! (RR-5): it writes 3 files into the folder of its attempt,
//! `<fleet root>/tasks/<task>/attempts/<n>/`, and fleetd reads them
//! (`docs/specs/fleetd/design.md`, "Workspace layout and file protocol",
//! protocol rules 1 to 3):
//!
//! - [`HEARTBEAT_JSON`]: a snapshot, every `heartbeat_every_s`, with a `seq`
//!   that grows by 1 on each write (RR-8).
//! - [`EXIT_JSON`]: the exit of the agent, 1 time.
//! - [`SESSION_JSON`]: the session id of the CLI and its harness, 1 time,
//!   when the wrapper finds it.
//!
//! Each file is written as `<name>.tmp` with mode 0600, fsynced, renamed to
//! its name, and then the folder is fsynced. The wrapper opens the ledger
//! read-only, only to find its attempt. A worker with no attempt (an
//! operator spawn) writes no file. [`settle`] is the exit rule of the worker
//! row, for the code that applies the facts of `exit.json`.

use std::fs::{DirBuilder, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use super::model::{Actor, EventKind, WorkerStatus};
use super::store::Store;
use super::tasks;
use super::util::{self, About};
use super::workers;
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::adapters::task_files::attempt_dir;
use crate::fleetd::task_execution::domain::outbox::{DIR_MODE, FILE_MODE, TMP_SUFFIX};

/// The snapshot of the wrapper in the attempt folder.
pub const HEARTBEAT_JSON: &str = "heartbeat.json";
/// The exit of the agent in the attempt folder.
pub const EXIT_JSON: &str = "exit.json";
/// The session of the agent in the attempt folder.
pub const SESSION_JSON: &str = "session.json";

/// The content of [`HEARTBEAT_JSON`] (the example of the design). A fact
/// that the wrapper does not know is `null`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatFile {
    pub task: String,
    pub attempt: String,
    /// The PID of the agent, once it started.
    pub pid: Option<u32>,
    pub pgid: Option<u32>,
    /// The pane of the wrapper.
    pub mux_pane: Option<String>,
    /// `running` while the agent process lives, then `exited`.
    pub proc_state: Option<String>,
    pub agent_state: Option<String>,
    pub last_output_at: Option<String>,
    pub tokens: Option<Tokens>,
    pub seq: u64,
    pub written_at: String,
}

/// The context use of the agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub context: u64,
    pub limit: u64,
}

/// The content of [`EXIT_JSON`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitFile {
    pub task: String,
    pub attempt: String,
    /// The exit code; `null` when a signal ended the agent.
    pub code: Option<i32>,
    /// The wrapper ran the repair resume of FM-6 before the exit.
    pub repaired: bool,
    pub at: String,
    /// What the wrapper saw end the agent, when it saw a signal of the
    /// CLI. A hint from a process that the agent can reach: never a proof
    /// of success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_hint: Option<OutcomeHint>,
}

/// The outcome hint of [`EXIT_JSON`]. A closed list, so no hint can say
/// that a task is done: only the verified result does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeHint {
    /// The CLI showed its usage-limit or outage signal (RR-11).
    Quota,
}

/// The content of [`SESSION_JSON`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFile {
    pub task: String,
    pub attempt: String,
    pub session_id: String,
    /// The harness of the CLI, as the brief names it.
    pub harness: String,
    /// The transcript of the session, when the wrapper found it.
    pub transcript: Option<String>,
    /// The id of the execution record of the session, so that fleetd joins
    /// the worker row to it. A wrapper of an older version writes none.
    #[serde(default)]
    pub record_id: Option<String>,
    pub written_at: String,
}

/// What the wrapper knows of its agent. The launch and each heartbeat
/// writer of 1 wrapper share it, so the `seq` grows over the repair resume.
#[derive(Debug, Default)]
pub struct AgentFacts {
    /// 0 until the agent starts.
    pid: AtomicU32,
    /// The `seq` of the last heartbeat.
    seq: AtomicU64,
    mux_pane: Option<String>,
    /// The time of the last activity of the agent: `last_output_at`.
    last_output_at: Mutex<Option<String>>,
    /// Held over 1 heartbeat write, so 2 writers of 1 wrapper do not mix
    /// their temporary file or their `seq` order.
    beat: Mutex<()>,
}

impl AgentFacts {
    pub fn new(mux_pane: Option<String>) -> Arc<AgentFacts> {
        Arc::new(AgentFacts {
            mux_pane,
            ..AgentFacts::default()
        })
    }

    /// The agent process `pid` started.
    pub fn agent_started(&self, pid: u32) {
        self.pid.store(pid, Ordering::SeqCst);
    }

    /// The agent showed activity now: an event of its stream.
    pub fn activity(&self) {
        *lock(&self.last_output_at) = Some(util::now());
    }

    fn pid(&self) -> Option<u32> {
        Some(self.pid.load(Ordering::SeqCst)).filter(|&p| p != 0)
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// Lock `m`. A writer that panicked left a whole value: take it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The wrapper files of 1 attempt.
#[derive(Clone, Debug)]
pub struct AttemptFiles {
    attempt: AttemptRef,
    dir: PathBuf,
}

impl AttemptFiles {
    pub fn new(fleet_root: &Path, attempt: AttemptRef) -> AttemptFiles {
        let dir = attempt_dir(fleet_root, &attempt.task_id, attempt.attempt());
        AttemptFiles { attempt, dir }
    }

    /// The files of the newest attempt of `worker_id` in the ledger `db`,
    /// which it opens read-only. `None`: the worker has no attempt.
    pub fn of_worker(db: &Path, worker_id: &str, fleet_root: &Path) -> Result<Option<Self>> {
        let store = Store::open_read_only(db)?;
        let row: Option<(String, String)> = store
            .conn()
            .query_row(
                "SELECT id, task_id FROM attempts WHERE worker_id = ?1 AND task_id IS NOT NULL
                 ORDER BY rowid DESC LIMIT 1",
                [worker_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row.map(|(attempt_id, task_id)| {
            AttemptFiles::new(
                fleet_root,
                AttemptRef {
                    task_id,
                    attempt_id,
                },
            )
        }))
    }

    /// The attempt folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn task(&self) -> String {
        self.attempt.task_id.clone()
    }

    fn attempt(&self) -> String {
        self.attempt.attempt().to_string()
    }

    /// Replace [`HEARTBEAT_JSON`] with the facts of now and the next `seq`.
    pub fn write_heartbeat(&self, facts: &AgentFacts) -> Result<()> {
        let _beat = lock(&facts.beat);
        let pid = facts.pid();
        let file = HeartbeatFile {
            task: self.task(),
            attempt: self.attempt(),
            pid,
            pgid: None,
            mux_pane: facts.mux_pane.clone(),
            proc_state: pid.map(|p| {
                let state = if horch_platform::process::alive(p) {
                    "running"
                } else {
                    "exited"
                };
                state.to_string()
            }),
            agent_state: None,
            last_output_at: lock(&facts.last_output_at).clone(),
            tokens: None,
            seq: facts.next_seq(),
            written_at: util::now(),
        };
        write_file(&self.dir, HEARTBEAT_JSON, &serde_json::to_vec(&file)?, true)?;
        Ok(())
    }

    /// Write [`EXIT_JSON`]. A second exit is refused.
    pub fn write_exit(&self, code: Option<i32>, repaired: bool) -> Result<()> {
        self.write_exit_hinted(code, repaired, None)
    }

    /// [`AttemptFiles::write_exit`] with the outcome hint `hint`.
    pub fn write_exit_hinted(
        &self,
        code: Option<i32>,
        repaired: bool,
        hint: Option<OutcomeHint>,
    ) -> Result<()> {
        let file = ExitFile {
            task: self.task(),
            attempt: self.attempt(),
            code,
            repaired,
            at: util::now(),
            outcome_hint: hint,
        };
        if !write_file(&self.dir, EXIT_JSON, &serde_json::to_vec(&file)?, false)? {
            bail!(
                "{} exists; the exit of an attempt is written 1 time",
                self.dir.join(EXIT_JSON).display()
            );
        }
        Ok(())
    }

    /// Write [`SESSION_JSON`] 1 time. False: it exists, and stays.
    pub fn write_session(
        &self,
        session_id: &str,
        harness: &str,
        transcript: Option<&str>,
        record_id: Option<&str>,
    ) -> Result<bool> {
        let file = SessionFile {
            task: self.task(),
            attempt: self.attempt(),
            session_id: session_id.to_string(),
            harness: harness.to_string(),
            transcript: transcript.map(str::to_string),
            record_id: record_id.map(str::to_string),
            written_at: util::now(),
        };
        write_file(&self.dir, SESSION_JSON, &serde_json::to_vec(&file)?, false)
    }
}

/// Write `bytes` to `dir/name` through `name.tmp`: mode 0600, fsync,
/// rename, fsync of `dir`. With `replace` false, an existing file stays and
/// the result is false. A link at the temp name is removed, not followed.
fn write_file(dir: &Path, name: &str, bytes: &[u8], replace: bool) -> Result<bool> {
    make_dir(dir)?;
    let path = dir.join(name);
    if !replace && path.symlink_metadata().is_ok() {
        return Ok(false);
    }
    let tmp = dir.join(format!("{name}{TMP_SUFFIX}"));
    match std::fs::remove_file(&tmp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            return Err(e).with_context(|| format!("remove {}", tmp.display()));
        }
        _ => {}
    }
    let renamed = write_temp(&tmp, bytes).and_then(|()| {
        if replace {
            std::fs::rename(&tmp, &path)
        } else {
            horch_platform::fs::rename_noreplace(&tmp, &path)
        }
    });
    if renamed.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    match renamed {
        Err(e) if !replace && e.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
        r => r.with_context(|| format!("write {}", path.display()))?,
    }
    ok_if_not_supported(horch_platform::fs::sync_dir(dir))
        .with_context(|| format!("sync {}", dir.display()))?;
    Ok(true)
}

/// Create `tmp` (never through a link) with `bytes` and mode 0600, synced.
fn write_temp(tmp: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    // Where the OS has no modes, the file gets the default permissions.
    let _ = horch_platform::fs::create_mode(&mut options, FILE_MODE);
    let mut file = options.open(tmp)?;
    // The umask may have cleared bits of the mode.
    ok_if_not_supported(horch_platform::fs::set_mode(tmp, FILE_MODE))?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Make the attempt folder with mode 0700 when it is missing.
fn make_dir(dir: &Path) -> Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    let _ = horch_platform::fs::create_dir_mode(&mut builder, DIR_MODE);
    builder
        .create(dir)
        .with_context(|| format!("make {}", dir.display()))
}

/// An `Unsupported` error of the platform (Windows: no modes, no dir sync)
/// is not a failure of the write.
fn ok_if_not_supported(r: io::Result<()>) -> io::Result<()> {
    match r {
        Err(e) if e.kind() == io::ErrorKind::Unsupported => Ok(()),
        r => r,
    }
}

/// Write [`EXIT_JSON`] of the newest attempt of `worker_id`. False: the
/// worker has no attempt, and no file is written.
pub fn write_exit(
    db: &Path,
    worker_id: &str,
    fleet_root: &Path,
    code: Option<i32>,
    repaired: bool,
) -> Result<bool> {
    let Some(files) = AttemptFiles::of_worker(db, worker_id, fleet_root)? else {
        return Ok(false);
    };
    files.write_exit(code, repaired)?;
    Ok(true)
}

/// Looks up the session id and transcript path while the agent runs.
pub type SessionLookup = Box<dyn Fn() -> (Option<String>, Option<String>) + Send>;

/// The checkin thread. Dropping it, or [`Checkins::stop`], ends it within 1
/// tick.
pub struct Checkins {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Checkins {
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Checkins {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The wrapper heartbeat period in seconds: `heartbeat_every_s` of
/// `fleet.toml` in `fleet_root`. A file that cannot be read gives the
/// default, with 1 line that names the file: a heartbeat never stops for a
/// config error.
pub fn fleet_period(fleet_root: &Path) -> (u64, Option<String>) {
    use crate::fleetd::fleet_runtime::adapters::config_file;
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
    match config_file::load(fleet_root) {
        Ok(config) => (config.heartbeat_every_s, None),
        Err(e) => {
            let secs = FleetConfig::default().heartbeat_every_s;
            let text = format!("{e:#}").replace('\n', " ");
            (
                secs,
                Some(format!(
                    "horch worker: {text}; the heartbeat runs every {secs} s"
                )),
            )
        }
    }
}

/// The checkin interval of the worker: the [`fleet_period`], or the run's
/// `heartbeat_every_s` when it is shorter, so no run beats more slowly than
/// before.
pub fn interval(db: &Path, worker_id: &str, fleet_root: &Path) -> Duration {
    let (fleet, warning) = fleet_period(fleet_root);
    if let Some(line) = warning {
        eprintln!("{line}");
    }
    let run = Store::open_read_only(db).ok().and_then(|s| {
        s.conn()
            .query_row(
                "SELECT r.heartbeat_every_s FROM workers w JOIN runs r ON r.run_id = w.run_id
                 WHERE w.worker_id = ?1",
                [worker_id],
                |r| r.get::<_, i64>(0),
            )
            .ok()
    });
    let secs = run.map_or(fleet, |run| fleet.min(run.max(1) as u64)).max(1);
    Duration::from_secs(secs)
}

/// The heartbeat writer of 1 wrapper: [`HEARTBEAT_JSON`] on each beat, and
/// [`SESSION_JSON`] 1 time when `lookup` knows the session id.
pub struct HeartbeatWriter {
    db: PathBuf,
    worker_id: String,
    fleet_root: PathBuf,
    /// The files of the attempt, once the ledger has it.
    files: Option<AttemptFiles>,
    facts: Arc<AgentFacts>,
    harness: String,
    lookup: SessionLookup,
    session_written: bool,
    /// The beat of a worker with no attempt, from the caller.
    no_attempt: Option<NoAttemptBeat>,
    /// The id of the execution record of the wrapper, for [`SESSION_JSON`].
    record_id: Option<String>,
}

/// The beat of a worker that has no attempt, so no attempt folder.
pub type NoAttemptBeat = Box<dyn FnMut() -> Result<()> + Send>;

impl HeartbeatWriter {
    pub fn new(
        db: PathBuf,
        worker_id: String,
        fleet_root: PathBuf,
        facts: Arc<AgentFacts>,
        harness: String,
        lookup: SessionLookup,
    ) -> HeartbeatWriter {
        HeartbeatWriter {
            db,
            worker_id,
            fleet_root,
            files: None,
            facts,
            harness,
            lookup,
            session_written: false,
            no_attempt: None,
            record_id: None,
        }
    }

    /// Write `record_id`, the id of the execution record of the wrapper,
    /// in [`SESSION_JSON`].
    pub fn with_record_id(mut self, record_id: String) -> HeartbeatWriter {
        self.record_id = Some(record_id);
        self
    }

    /// Run `beat` on each beat while the worker has no attempt.
    pub fn with_no_attempt_beat(mut self, beat: NoAttemptBeat) -> HeartbeatWriter {
        self.no_attempt = Some(beat);
        self
    }

    /// 1 beat. A worker with no attempt yet writes no file: it runs the
    /// beat of [`HeartbeatWriter::with_no_attempt_beat`], if any.
    pub fn beat(&mut self) -> Result<()> {
        let files = match &self.files {
            Some(files) => files,
            None => match AttemptFiles::of_worker(&self.db, &self.worker_id, &self.fleet_root)? {
                Some(files) => self.files.insert(files),
                None => return self.no_attempt.as_mut().map_or(Ok(()), |beat| beat()),
            },
        };
        files.write_heartbeat(&self.facts)?;
        if !self.session_written {
            if let (Some(session), transcript) = (self.lookup)() {
                files.write_session(
                    &session,
                    &self.harness,
                    transcript.as_deref(),
                    self.record_id.as_deref(),
                )?;
                self.session_written = true;
            }
        }
        Ok(())
    }
}

/// Beat `writer` every `every` until stopped.
pub fn start_heartbeats(mut writer: HeartbeatWriter, every: Duration) -> Checkins {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::spawn(move || {
        let tick = Duration::from_millis(100).min(every);
        let mut since = Duration::ZERO;
        let mut log = CheckinLog::default();
        loop {
            if flag.load(Ordering::SeqCst) {
                return;
            }
            if since >= every {
                since = Duration::ZERO;
                let result = writer.beat().map_err(|e| format!("{e:#}"));
                if let Some(line) = log.record(result, Instant::now()) {
                    eprintln!("{line}");
                }
            }
            std::thread::sleep(tick);
            since += tick;
        }
    });
    Checkins {
        stop,
        thread: Some(thread),
    }
}

/// The longest time 1 repeated checkin error stays silent.
pub const REPEAT_EVERY: Duration = Duration::from_secs(10 * 60);

/// What the checkin thread printed, so 1 error that repeats every checkin
/// does not fill the pane (TRF-49).
#[derive(Debug, Default)]
pub struct CheckinLog {
    /// The text of the last failure, while checkins fail.
    last_error: Option<String>,
    /// How many checkins in a row failed with `last_error`.
    repeats: u64,
    /// How many checkins in a row failed, with any text.
    failures: u64,
    /// When the last failure line was printed.
    printed_at: Option<Instant>,
}

impl CheckinLog {
    /// The line to print for 1 checkin `result` at `now`, if any. A failure
    /// prints the first time and when its text changes. The same failure
    /// prints again only after [`REPEAT_EVERY`], with its repeat count. The
    /// first success after failures prints 1 recovery line.
    pub fn record(&mut self, result: Result<(), String>, now: Instant) -> Option<String> {
        let text = match result {
            Ok(()) => {
                let failures = std::mem::take(self).failures;
                return (failures > 0)
                    .then(|| format!("horch worker: checkin recovered after {failures} failures"));
            }
            Err(text) => text,
        };
        self.failures += 1;
        if self.last_error.as_deref() != Some(text.as_str()) {
            self.last_error = Some(text.clone());
            self.repeats = 1;
            self.printed_at = Some(now);
            return Some(format!("horch worker: checkin failed: {text}"));
        }
        self.repeats += 1;
        let due = self
            .printed_at
            .is_none_or(|at| now.saturating_duration_since(at) >= REPEAT_EVERY);
        if !due {
            return None;
        }
        self.printed_at = Some(now);
        Some(format!(
            "horch worker: checkin failed {n} times in a row: {text}",
            n = self.repeats
        ))
    }
}

/// How a worker's agent ended, for the supervisor's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// It ran `horch ledger done`, or held no task.
    Clean,
    /// It held a task without finishing it: the task is queued again.
    Requeued { task: String, persona: String },
    /// It held a task that has now used up its attempts: the task is
    /// `failed` and waits for its 1 replan by the orchestrator (rung 6).
    Failed { task: String },
    /// It held a task that used up its attempts after its replan: the task
    /// is `escalated`, held for the operator (#168).
    Escalated { task: String },
}

/// Settle the worker row of `worker_id` after its agent exited with
/// `code`, in the transaction `tx`: the rule for the facts of
/// [`EXIT_JSON`].
pub fn settle(tx: &rusqlite::Connection, worker_id: &str, code: Option<i32>) -> Result<Ended> {
    let w = workers::get(tx, worker_id)?;
    let status = w.status()?;
    let about = About {
        run: w.run_id.as_deref(),
        task: w.task_id.as_deref(),
        worker: Some(worker_id),
    };
    let reason = record_exit(tx, worker_id, status, code)?;
    util::event(tx, &about, Actor::Supervisor, EventKind::Exited, &reason)?;
    if status == WorkerStatus::Working {
        return requeue_after_exit(tx, &w, &reason);
    }
    Ok(Ended::Clean)
}

/// End the worker per [`exit_rule`], or only record the exit code. Returns
/// the reason text of the exit event.
fn record_exit(
    tx: &rusqlite::Connection,
    worker_id: &str,
    status: WorkerStatus,
    code: Option<i32>,
) -> Result<String> {
    let how = exit_text(code);
    let Some((end, suffix)) = exit_rule(status) else {
        workers::set_exit_code(tx, worker_id, code)?;
        return Ok(how);
    };
    let reason = format!("{how}{suffix}");
    workers::end(tx, worker_id, end, &reason, code)?;
    Ok(reason)
}

/// How the agent ended, as the exit event words it.
fn exit_text(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("agent exited with code {c}"),
        None => "agent was ended by a signal".to_string(),
    }
}

/// The end status and reason suffix of a worker whose agent exited in
/// `status`; `None` keeps the row and only records the exit code.
fn exit_rule(status: WorkerStatus) -> Option<(WorkerStatus, &'static str)> {
    match status {
        WorkerStatus::Working => Some((WorkerStatus::Incomplete, " before horch ledger done")),
        WorkerStatus::Starting | WorkerStatus::Spawning => Some((WorkerStatus::LaunchFailed, "")),
        WorkerStatus::Claiming => Some((WorkerStatus::NoTask, " before it claimed a task")),
        _ => None,
    }
}

/// Queue the task of a worker that exited while working, or hold it.
fn requeue_after_exit(
    tx: &rusqlite::Connection,
    w: &workers::Worker,
    reason: &str,
) -> Result<Ended> {
    let task = w.task_id.clone().unwrap_or_default();
    let how = tasks::requeue(tx, &task, &format!("{} {reason}", w.label()))?;
    let t = tasks::get(tx, &task)?;
    Ok(match how {
        tasks::Requeue::Failed => Ended::Failed { task: t.short() },
        tasks::Requeue::Escalated => Ended::Escalated { task: t.short() },
        _ => Ended::Requeued {
            task: t.short(),
            persona: t.claimable_by.unwrap_or_default(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::workers::NewWorker;
    use rusqlite::params;

    struct Fixture {
        _dir: tempfile::TempDir,
        db: PathBuf,
        store: Store,
        worker: String,
    }

    /// A worker in `status` (`working` holds a claimed task).
    fn fixture(status: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("l.sqlite3");
        let mut store = Store::open(&db).unwrap();
        let worker = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/r"),
                    &NewRun {
                        slug: "s".into(),
                        ..Default::default()
                    },
                )?;
                let id = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "opus".into(),
                        role: Some("opus-x".into()),
                        ..Default::default()
                    },
                )?;
                if status == "working" {
                    tasks::add(
                        tx,
                        &run,
                        &NewTask::work("a", "tickets/a.md", "opus"),
                        Actor::Orchestrator,
                    )?;
                    tasks::claim(tx, &id)?;
                }
                tx.execute(
                    "UPDATE workers SET status = ?2 WHERE worker_id = ?1 AND status != 'working'",
                    params![id, status],
                )?;
                Ok(id)
            })
            .unwrap();
        Fixture {
            _dir: dir,
            db,
            store,
            worker,
        }
    }

    /// Settle the worker as the code that applies `exit.json` does.
    fn agent_exited(db: &Path, worker_id: &str, code: Option<i32>) -> Result<Ended> {
        Store::open(db)?.immediate(|tx| settle(tx, worker_id, code))
    }

    fn row(f: &Fixture) -> (String, Option<String>, Option<i64>) {
        f.store
            .conn()
            .query_row(
                "SELECT status, end_reason, exit_code FROM workers WHERE worker_id = ?1",
                [&f.worker],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    }

    fn exited_text(f: &Fixture) -> Vec<String> {
        let mut q = f
            .store
            .conn()
            .prepare("SELECT text FROM events WHERE worker_id = ?1 AND kind = 'exited'")
            .unwrap();
        let rows = q.query_map([&f.worker], |r| r.get(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    #[test]
    fn a_working_agent_that_exits_ends_incomplete_and_requeues_its_task() {
        let f = fixture("working");
        let ended = agent_exited(&f.db, &f.worker, Some(3)).unwrap();
        assert_eq!(
            ended,
            Ended::Requeued {
                task: "T-1".into(),
                persona: "opus".into()
            }
        );
        let reason = "agent exited with code 3 before horch ledger done";
        assert_eq!(row(&f), ("incomplete".into(), Some(reason.into()), Some(3)));
        assert_eq!(exited_text(&f), [reason]);
        let status: String = f
            .store
            .conn()
            .query_row("SELECT status FROM tasks WHERE seq = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "ready");
    }

    #[test]
    fn a_starting_or_spawning_agent_that_exits_is_a_failed_launch() {
        for status in ["starting", "spawning"] {
            let f = fixture(status);
            assert_eq!(agent_exited(&f.db, &f.worker, None).unwrap(), Ended::Clean);
            let how = "agent was ended by a signal";
            assert_eq!(row(&f), ("launch_failed".into(), Some(how.into()), None));
            assert_eq!(exited_text(&f), [how]);
        }
    }

    #[test]
    fn a_claiming_agent_that_exits_ends_with_no_task() {
        let f = fixture("claiming");
        assert_eq!(
            agent_exited(&f.db, &f.worker, Some(0)).unwrap(),
            Ended::Clean
        );
        let reason = "agent exited with code 0 before it claimed a task";
        assert_eq!(row(&f), ("no_task".into(), Some(reason.into()), Some(0)));
        assert_eq!(exited_text(&f), [reason]);
    }

    /// Set the run's `heartbeat_every_s` of the fixture's worker.
    fn set_run_period(f: &Fixture, secs: i64) {
        f.store
            .conn()
            .execute(
                "UPDATE runs SET heartbeat_every_s = ?1
                 WHERE run_id = (SELECT run_id FROM workers WHERE worker_id = ?2)",
                params![secs, f.worker],
            )
            .unwrap();
    }

    #[test]
    fn interval_is_heartbeat_every_s_of_fleet_toml() {
        let f = fixture("working");
        let fleet = tempfile::tempdir().unwrap();
        assert_eq!(
            interval(&f.db, &f.worker, fleet.path()),
            Duration::from_secs(10),
            "no fleet.toml gives the design's 10 s"
        );
        std::fs::write(fleet.path().join("fleet.toml"), "heartbeat_every_s = 7\n").unwrap();
        assert_eq!(
            interval(&f.db, &f.worker, fleet.path()),
            Duration::from_secs(7)
        );
    }

    #[test]
    fn interval_keeps_the_shorter_run_period() {
        let f = fixture("working");
        let fleet = tempfile::tempdir().unwrap();
        set_run_period(&f, 4);
        assert_eq!(
            interval(&f.db, &f.worker, fleet.path()),
            Duration::from_secs(4)
        );
        set_run_period(&f, 30);
        assert_eq!(
            interval(&f.db, &f.worker, fleet.path()),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn unreadable_fleet_toml_gives_the_default_period() {
        let f = fixture("working");
        let fleet = tempfile::tempdir().unwrap();
        let file = fleet.path().join("fleet.toml");
        // A value out of range, then a file that is a folder.
        std::fs::write(&file, "heartbeat_every_s = 0\n").unwrap();
        let (secs, warning) = fleet_period(fleet.path());
        assert_eq!(secs, 10);
        let warning = warning.expect("an unreadable fleet.toml prints 1 line");
        assert!(warning.contains(&file.display().to_string()), "{warning}");
        assert!(!warning.contains('\n'), "{warning}");
        assert_eq!(
            interval(&f.db, &f.worker, fleet.path()),
            Duration::from_secs(10)
        );
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        let (secs, warning) = fleet_period(fleet.path());
        assert_eq!(secs, 10);
        assert!(warning.unwrap().contains(&file.display().to_string()));
    }

    #[test]
    fn an_agent_of_a_settled_worker_only_records_its_exit_code() {
        let f = fixture("done");
        assert_eq!(
            agent_exited(&f.db, &f.worker, Some(7)).unwrap(),
            Ended::Clean
        );
        assert_eq!(row(&f), ("done".into(), None, Some(7)));
        assert_eq!(exited_text(&f), ["agent exited with code 7"]);
    }

    #[test]
    fn trf_49_same_error_prints_once_then_every_10_min() {
        let mut log = CheckinLog::default();
        let t0 = Instant::now();
        let err = || Err("the task ledger is schema version 8".to_string());
        assert_eq!(
            log.record(err(), t0).as_deref(),
            Some("horch worker: checkin failed: the task ledger is schema version 8")
        );
        // 1 checkin each 30 s for 10 min: silent until the 10 min mark.
        let mut printed = Vec::new();
        for i in 1..=40u64 {
            if let Some(line) = log.record(err(), t0 + Duration::from_secs(30 * i)) {
                printed.push((i, line));
            }
        }
        assert_eq!(
            printed,
            vec![
                (
                    20,
                    "horch worker: checkin failed 21 times in a row: the task ledger is schema version 8"
                        .to_string()
                ),
                (
                    40,
                    "horch worker: checkin failed 41 times in a row: the task ledger is schema version 8"
                        .to_string()
                ),
            ]
        );
    }

    #[test]
    fn trf_49_changed_error_prints() {
        let mut log = CheckinLog::default();
        let t0 = Instant::now();
        assert!(log.record(Err("a".into()), t0).is_some());
        assert!(log
            .record(Err("a".into()), t0 + Duration::from_secs(1))
            .is_none());
        assert_eq!(
            log.record(Err("b".into()), t0 + Duration::from_secs(2))
                .as_deref(),
            Some("horch worker: checkin failed: b")
        );
        assert!(log
            .record(Err("b".into()), t0 + Duration::from_secs(3))
            .is_none());
        // Back to the first text: it changed again, so it prints.
        assert_eq!(
            log.record(Err("a".into()), t0 + Duration::from_secs(4))
                .as_deref(),
            Some("horch worker: checkin failed: a")
        );
    }

    #[test]
    fn trf_49_recovery_prints_1_line() {
        let mut log = CheckinLog::default();
        let t0 = Instant::now();
        assert_eq!(log.record(Ok(()), t0), None, "no failure, no line");
        for (i, text) in ["a", "a", "b"].into_iter().enumerate() {
            log.record(Err(text.into()), t0 + Duration::from_secs(i as u64));
        }
        assert_eq!(
            log.record(Ok(()), t0 + Duration::from_secs(5)).as_deref(),
            Some("horch worker: checkin recovered after 3 failures")
        );
        assert_eq!(log.record(Ok(()), t0 + Duration::from_secs(6)), None);
        // A new failure after the recovery prints again.
        assert_eq!(
            log.record(Err("a".into()), t0 + Duration::from_secs(7))
                .as_deref(),
            Some("horch worker: checkin failed: a")
        );
    }
}
