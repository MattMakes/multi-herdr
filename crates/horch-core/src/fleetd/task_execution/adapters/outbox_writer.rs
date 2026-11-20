//! The outbox writer of a worker command: find the own attempt by the
//! process chain, make the outbox when it is missing, and write each file
//! atomically.
//!
//! Requirements: FR-7, RR-5, RR-8, SR-6. `docs/specs/fleetd/design.md`,
//! "Workspace layout and file protocol", protocol rules 1, 3 and 7;
//! `docs/tickets/fleetd-p5/MAP.md`, Q3 and Q4.
//!
//! - The own attempt: the writer reads the live attempts from the ledger,
//!   read-only, and takes the 1 attempt whose wrapper (PID and start time)
//!   is in the parent chain of this process. It reads no environment
//!   variable, so a worker cannot name the attempt of another worker. No
//!   such attempt, or a process fact that cannot be read, is a refusal:
//!   [`NO_LIVE_ATTEMPT`]. While an attempt has no recorded wrapper yet,
//!   the writer reads the ledger again before it refuses, for at most the
//!   start timeout of a launch.
//! - The outbox: made with mode 0700 when it is missing. Each file gets mode
//!   0600. A link in the place of the outbox or of a file is refused.
//! - A message (`result.json`, a question file): a temp file `*.tmp`, fsync,
//!   then a hard link to its name, which fails when the name exists, so a
//!   message is never rewritten; then the fsync of the folder.
//! - A log line (`progress.jsonl`): 1 `write` of the whole line, with
//!   `O_APPEND`, so the lines of 2 processes never interleave.
//!
//! The writer writes no ledger row. fleetd checks each file again at ingest
//! (ticket 29).
//!
//! Windows gap: the platform gives no parent PID on Windows, so each
//! command is refused there.

use std::fs::{DirBuilder, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::peer_process::{ProcessFacts, MAX_CHAIN};
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::pass::START_TIMEOUT_S;
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::domain::outbox::{
    question_name, question_number, LineKind, ProgressLine, Question, DIR_MODE, FILE_MODE,
    PROGRESS_JSONL, QUESTIONS, RESULT_JSON, TMP_SUFFIX,
};
use crate::fleetd::task_execution::domain::result::{validate, ResultFile};

/// The refusal of a process that is in no live attempt.
pub const NO_LIVE_ATTEMPT: &str = "no live attempt of this process";

/// The most question numbers that 1 write tries after the highest one it
/// saw, when other writers take them first.
const QUESTION_TRIES: u32 = 64;

/// A live attempt and the process of its wrapper, from the ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveAttempt {
    pub attempt: AttemptRef,
    pub wrapper_pid: u32,
    /// A start time of [`horch_platform::process::start_time`].
    pub wrapper_start: u64,
}

/// The live attempts of the ledger `conn` with a task and a recorded
/// wrapper. It only reads.
pub fn live_attempts(conn: &Connection) -> Result<Vec<LiveAttempt>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, pid, pid_start FROM attempts
         WHERE state = 'live' AND task_id IS NOT NULL
           AND pid IS NOT NULL AND pid_start IS NOT NULL
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let mut live = Vec::new();
    for row in rows {
        let (attempt_id, task_id, pid, start) = row.context("read the live attempts")?;
        // A value that is not a PID or a start time names no process.
        let (Ok(wrapper_pid), Ok(wrapper_start)) = (u32::try_from(pid), u64::try_from(start))
        else {
            continue;
        };
        live.push(LiveAttempt {
            attempt: AttemptRef {
                task_id,
                attempt_id,
            },
            wrapper_pid,
            wrapper_start,
        });
    }
    Ok(live)
}

/// Why a process has no own attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoLiveAttempt {
    /// The fact that is missing, when a fact could not be read or the
    /// chain holds more than 1 live attempt.
    pub detail: Option<String>,
}

impl std::fmt::Display for NoLiveAttempt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            None => f.write_str(NO_LIVE_ATTEMPT),
            Some(detail) => write!(f, "{NO_LIVE_ATTEMPT}: {detail}"),
        }
    }
}

impl std::error::Error for NoLiveAttempt {}

fn unknown(detail: String) -> NoLiveAttempt {
    NoLiveAttempt {
        detail: Some(detail),
    }
}

/// The 1 attempt of `live` whose wrapper is `pid` or a process in the
/// parent chain of `pid`, with the PID and the start time of the row.
pub fn own_attempt(
    pid: u32,
    live: &[LiveAttempt],
    facts: &dyn ProcessFacts,
) -> Result<AttemptRef, NoLiveAttempt> {
    let mut found: Vec<&LiveAttempt> = Vec::new();
    let mut p = pid;
    for _ in 0..MAX_CHAIN {
        for a in live.iter().filter(|a| a.wrapper_pid == p) {
            let start = facts
                .start_time(p)
                .ok_or_else(|| unknown(format!("the start time of process {p} cannot be read")))?;
            if a.wrapper_start == start {
                found.push(a);
            }
        }
        if p == 1 {
            return the_one(&found);
        }
        match facts.parent(p) {
            None => return Err(unknown(format!("the parent of process {p} cannot be read"))),
            Some(0) => return the_one(&found),
            Some(parent) => p = parent,
        }
    }
    Err(unknown(format!(
        "the parent chain is longer than {MAX_CHAIN} processes"
    )))
}

fn the_one(found: &[&LiveAttempt]) -> Result<AttemptRef, NoLiveAttempt> {
    match found {
        [] => Err(NoLiveAttempt { detail: None }),
        [one] => Ok(one.attempt.clone()),
        more => {
            let ids: Vec<&str> = more.iter().map(|a| a.attempt.attempt_id.as_str()).collect();
            Err(unknown(format!(
                "{} live attempts are in its parent chain: {}",
                ids.len(),
                ids.join(", ")
            )))
        }
    }
}

/// The longest time that a command waits for its attempt to record its
/// wrapper: the start timeout of a launch.
pub const ATTEMPT_WAIT: Duration = Duration::from_secs(START_TIMEOUT_S.unsigned_abs());

/// The time between 2 reads of the ledger while a command waits.
pub const ATTEMPT_POLL: Duration = Duration::from_millis(250);

/// The own attempt of this process, as [`own_attempt`] finds it. A push
/// attempt records its wrapper only when fleetd ingests a heartbeat with
/// the agent PID, and the agent can run a command before that. So while
/// the ledger has an attempt that has no wrapper yet, a process with no
/// own attempt reads the ledger again, for at most `wait`.
fn wait_own_attempt(
    ledger: &Connection,
    facts: &dyn ProcessFacts,
    wait: Duration,
) -> Result<AttemptRef> {
    let until = Instant::now() + wait;
    loop {
        let live = live_attempts(ledger)?;
        match own_attempt(std::process::id(), &live, facts) {
            Err(e) if e.detail.is_none() && Instant::now() < until && starting(ledger)? => {
                std::thread::sleep(ATTEMPT_POLL);
            }
            found => return Ok(found?),
        }
    }
}

/// Whether the ledger `conn` has an attempt that has no recorded wrapper
/// yet: `launching`, or `live` with no PID. An attempt of a task and an
/// attempt with no task (a sub-orchestrator) both count. It only reads.
pub fn starting(conn: &Connection) -> Result<bool> {
    let found: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM attempts
           WHERE state = 'launching' OR (state = 'live' AND pid IS NULL))",
        [],
        |r| r.get(0),
    )?;
    Ok(found)
}

/// The writer of the outbox of the own attempt of this process.
#[derive(Debug)]
pub struct OutboxWriter {
    attempt: AttemptRef,
    outbox: PathBuf,
    result_max_bytes: u64,
    question_max_bytes: u64,
    progress_line_max_bytes: u64,
}

impl OutboxWriter {
    /// The writer of the own attempt of this process, under `fleet_root`.
    /// `ledger` is the ledger, opened read-only by the caller. `facts` are
    /// the process facts: [`PlatformFacts`] outside a test.
    ///
    /// [`PlatformFacts`]: crate::fleetd::fleet_runtime::adapters::peer_process::PlatformFacts
    pub fn of_this_process(
        fleet_root: &Path,
        ledger: &Connection,
        config: &FleetConfig,
        facts: &dyn ProcessFacts,
    ) -> Result<OutboxWriter> {
        let attempt = wait_own_attempt(ledger, facts, ATTEMPT_WAIT)?;
        Ok(OutboxWriter {
            outbox: attempt.outbox(fleet_root),
            attempt,
            result_max_bytes: config.result_max_bytes,
            question_max_bytes: config.question_max_bytes,
            progress_line_max_bytes: config.progress_line_max_bytes,
        })
    }

    /// The own attempt.
    pub fn attempt(&self) -> &AttemptRef {
        &self.attempt
    }

    /// The outbox of the own attempt.
    pub fn outbox(&self) -> &Path {
        &self.outbox
    }

    /// Append 1 line of `kind` with `text` at the time `at` to
    /// `progress.jsonl`. A line over `progress_line_max_bytes` is refused
    /// before the write.
    pub fn append(&self, kind: LineKind, at: &str, text: &str) -> Result<()> {
        let line = ProgressLine {
            task: self.attempt.task_id.clone(),
            attempt: self.attempt.attempt().to_string(),
            at: at.to_string(),
            kind,
            text: text.to_string(),
        }
        .to_line(self.progress_line_max_bytes)?;
        make_dir(&self.outbox)?;
        let path = self.outbox.join(PROGRESS_JSONL);
        refuse_link(&path)?;
        let mut options = OpenOptions::new();
        options.append(true).create(true);
        // Where the OS has no modes, the file gets the default permissions.
        let _ = horch_platform::fs::create_mode(&mut options, FILE_MODE);
        let mut file = options
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        // 1 write of the whole line (protocol rule 1).
        let written = file
            .write(line.as_bytes())
            .with_context(|| format!("append to {}", path.display()))?;
        if written != line.len() {
            bail!(
                "{}: wrote {written} of {} bytes of 1 line",
                path.display(),
                line.len()
            );
        }
        Ok(())
    }

    /// Write `result.json` 1 time. `file` must pass the schema with the
    /// task and the attempt of the own attempt. Returns its path.
    pub fn write_result(&self, file: &ResultFile) -> Result<PathBuf> {
        let bytes = file.to_json().into_bytes();
        validate(
            &bytes,
            &self.attempt.task_id,
            self.attempt.attempt(),
            self.result_max_bytes,
        )?;
        make_dir(&self.outbox)?;
        let path = self.outbox.join(RESULT_JSON);
        match create_message(&self.outbox, RESULT_JSON, &bytes) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => bail!(
                "{} exists; result.json of an attempt is written 1 time",
                path.display()
            ),
            r => r.with_context(|| format!("write {}", path.display())),
        }
    }

    /// Write the next question file `questions/<n>.json` with `text` at the
    /// time `at`. Returns its path.
    pub fn write_question(&self, at: &str, text: &str) -> Result<PathBuf> {
        let bytes = Question {
            task: self.attempt.task_id.clone(),
            attempt: self.attempt.attempt().to_string(),
            at: at.to_string(),
            text: text.to_string(),
        }
        .to_json(self.question_max_bytes)?
        .into_bytes();
        make_dir(&self.outbox)?;
        let dir = self.outbox.join(QUESTIONS);
        make_dir(&dir)?;
        let first = highest_question(&dir)?
            .checked_add(1)
            .context("no question number is free")?;
        for n in first..first.saturating_add(QUESTION_TRIES) {
            match create_message(&dir, &question_name(n), &bytes) {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                r => {
                    return r.with_context(|| format!("write a question file in {}", dir.display()))
                }
            }
        }
        bail!(
            "{}: question numbers {first} to {} are taken",
            dir.display(),
            first.saturating_add(QUESTION_TRIES - 1)
        )
    }
}

/// The highest number of a question file in `dir`, 0 when it has none.
fn highest_question(dir: &Path) -> Result<u32> {
    let mut highest = 0;
    for entry in std::fs::read_dir(dir).with_context(|| format!("list {}", dir.display()))? {
        let entry = entry.with_context(|| format!("list {}", dir.display()))?;
        if let Some(n) = question_number(&entry.file_name().to_string_lossy()) {
            highest = highest.max(n);
        }
    }
    Ok(highest)
}

/// Make the folder `dir` with [`DIR_MODE`] when it is missing. Its parent
/// must exist. A link or a file in its place is refused.
fn make_dir(dir: &Path) -> Result<()> {
    if is_dir(dir)? {
        return Ok(());
    }
    let mut builder = DirBuilder::new();
    // Where the OS has no modes, the folder gets the default permissions.
    let _ = horch_platform::fs::create_dir_mode(&mut builder, DIR_MODE);
    match builder.create(dir) {
        Ok(()) => {
            // The umask may have cleared bits of the mode.
            ok_if_not_supported(horch_platform::fs::set_mode(dir, DIR_MODE))
                .with_context(|| format!("set the mode of {}", dir.display()))?;
            if let Some(parent) = dir.parent() {
                ok_if_not_supported(horch_platform::fs::sync_dir(parent))
                    .with_context(|| format!("sync {}", parent.display()))?;
            }
            Ok(())
        }
        // Another process made it first.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && is_dir(dir)? => Ok(()),
        Err(e) => Err(e).with_context(|| format!("make {}", dir.display())),
    }
}

/// `dir` is a folder, not a link. `false` when nothing is at `dir`.
fn is_dir(dir: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => Ok(true),
        Ok(_) => bail!("{} is not a folder", dir.display()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("read {}", dir.display())),
    }
}

/// Refuse a link at `path`.
fn refuse_link(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => bail!("{} is a link", path.display()),
        _ => Ok(()),
    }
}

/// Write the message `name` in `dir` 1 time: a temp file, fsync, a hard
/// link to `name`, then the fsync of `dir`. An `AlreadyExists` error when
/// `name` exists; the existing file stays as it is.
fn create_message(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let path = dir.join(name);
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let tmp = dir.join(format!(
        "{name}.{}-{}{TMP_SUFFIX}",
        std::process::id(),
        &nonce[..12]
    ));
    let linked = write_temp(&tmp, bytes).and_then(|()| std::fs::hard_link(&tmp, &path));
    let _ = std::fs::remove_file(&tmp);
    linked?;
    ok_if_not_supported(horch_platform::fs::sync_dir(dir))?;
    Ok(path)
}

/// Create the new file `tmp` with `bytes` and [`FILE_MODE`], synced to disk.
fn write_temp(tmp: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let _ = horch_platform::fs::create_mode(&mut options, FILE_MODE);
    let mut file = options.open(tmp)?;
    ok_if_not_supported(horch_platform::fs::set_mode(tmp, FILE_MODE))?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// A platform call that is `NotSupported` on this OS is not an error here:
/// the OS has no modes or cannot sync a folder.
fn ok_if_not_supported(r: io::Result<()>) -> io::Result<()> {
    match r {
        Err(e)
            if e.get_ref()
                .is_some_and(|inner| inner.is::<horch_platform::NotSupported>()) =>
        {
            Ok(())
        }
        r => r,
    }
}
