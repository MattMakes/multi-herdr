//! Reads the wrapper files of an attempt: `heartbeat.json`, `exit.json` and
//! `session.json`, each with the checks and inside its own attempt folder,
//! and writes the rows that the wrapper wrote before Phase 5.
//!
//! Requirements: RR-5, RR-8, SR-6. `docs/specs/fleetd/design.md`,
//! "Workspace layout and file protocol", protocol rules 2 and 7;
//! `docs/tickets/fleetd-p5/31-ingest-wrapper-files.md`.
//!
//! - Part 1 of a pass calls [`read`] for each attempt of its scan. Each file
//!   goes through the protected file reader with the attempt folder as its
//!   root and the size limit of its type, and its payload must name the
//!   task and the attempt of its path. A failed file is
//!   [`Found::Rejected`].
//! - Part 2 calls [`ingest`] in its transaction.
//! - `heartbeat.json` is a snapshot. fleetd does not ingest it: it uses it
//!   only when its `seq` is higher than `attempts.heartbeat_seq`, the last
//!   `seq` it used. A used snapshot sets `workers.last_checkin_at` (the
//!   checkin of the wrapper, Phase 2 Q6), the agent and wrapper PIDs that a
//!   stop uses, and `attempts.active_at` when the agent wrote output since
//!   the last checkin (Phase 2 Q7).
//! - `exit.json` and `session.json` are messages: 1 `ingested` row by path
//!   and sha256, and 1 effect. `exit.json` settles the worker row and ends
//!   its session record ([`supervisor::settle_exit`]); `session.json` fills
//!   the session id, the transcript and the execution record id of the
//!   worker row, which the session records show. A record id of another
//!   form rejects the file. `session.json` goes first in a pass, so the
//!   exit of the same pass finds the record.
//! - A file that fails a check gets 1 `ingested` row `rejected` with its
//!   reason, and fleetd does not read its path again (protocol rule 7).
//!
//! The PID of the agent is a claim of a file in the task folder, so fleetd
//! checks it: the process runs, its parent (the wrapper) runs, neither is
//! fleetd, and while a recorded wrapper runs, a new agent must be its
//! child. A fact that the platform does not give fails the check.
//!
//! The wrapper writes its first heartbeat before its agent starts, with
//! its own PID and no agent, and starts the agent only when fleetd has
//! recorded it (SR-8, FM-32). fleetd records such a wrapper only for an
//! attempt that it launched (its launch record exists) and that is not
//! ended, and only when the worker row has no wrapper or has this one.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Result;
use horch_platform::channel::User;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;

use crate::fleetd::fleet_runtime::adapters::effects;
use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::fleet_runtime::adapters::sqlite::{self as door, has_fact};
use crate::fleetd::fleet_runtime::application::ledger::{record, Cause, DoorError};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::task_execution::adapters::ingested_store as store;
use crate::fleetd::task_execution::adapters::leases_store;
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::adapters::task_files::attempt_dir;
use crate::fleetd::task_execution::domain::repair::{REPAIR_CAUSE, REPAIR_LINE};
use crate::fleetd::task_execution::domain::task::{AttemptState, Recorded};
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::supervisor::{
    self, Ended, ExitFile, HeartbeatFile, OutcomeHint, SessionFile, EXIT_JSON, HEARTBEAT_JSON,
    SESSION_JSON,
};
use crate::taskledger::{util, workers};

/// The cause of the note on an attempt whose `exit.json` has the outcome
/// hint `quota`: the wrapper saw the usage-limit signal of the CLI. Rung 5
/// then gives the outcome `quota` (RR-11).
pub const QUOTA_HINT: &str = "the wrapper saw a usage limit";

/// The size limit of `exit.json`.
pub const EXIT_MAX_BYTES: u64 = 4096;

/// The size limit of `session.json`.
pub const SESSION_MAX_BYTES: u64 = 4096;

/// What the reader found in 1 file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found<T> {
    /// The file passed each check.
    Valid { sha256: String, file: T },
    /// The file failed a check. `sha256` is known when the bytes were read.
    Rejected {
        sha256: Option<String>,
        reason: String,
    },
}

/// 1 wrapper file of an attempt and what the reader found in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrapperFile<T> {
    pub path: PathBuf,
    pub found: Found<T>,
}

/// The wrapper files that part 1 read for 1 attempt. `None`: no file, or a
/// path that the ledger has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrapperRead {
    pub attempt: AttemptRef,
    /// fleetd launched the attempt: its launch record exists.
    pub launched: bool,
    pub heartbeat: Option<WrapperFile<HeartbeatFile>>,
    pub exit: Option<WrapperFile<ExitFile>>,
    pub session: Option<WrapperFile<SessionFile>>,
}

/// What 1 ingest did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The attempts whose heartbeat was used.
    pub checkins: Vec<String>,
    /// The push attempts whose `exit.json` this ingest recorded with their
    /// worker row left live: the pass ends each by rung 5.
    pub exited: Vec<String>,
    /// The wrapper of each used heartbeat that passed the PID check, by
    /// attempt. The pass records it on the attempt row at `launching ->
    /// live`.
    pub wrappers: BTreeMap<String, Recorded>,
    /// The attempts whose exit settled their worker, with how it ended.
    pub exits: Vec<(String, Ended)>,
    /// The attempts whose session was recorded.
    pub sessions: Vec<String>,
    /// The files that this ingest recorded as rejected.
    pub rejected: Vec<PathBuf>,
}

/// The process facts that the PID check reads.
pub trait ProcessFacts {
    /// The start time of `pid`; `None` when no process has it.
    fn start_time(&self, pid: u32) -> Option<u64>;
    /// The parent of `pid`; `None` when it is not known.
    fn parent(&self, pid: u32) -> Option<u32>;
}

/// The processes of this machine.
pub struct OsProcesses;

impl ProcessFacts for OsProcesses {
    fn start_time(&self, pid: u32) -> Option<u64> {
        crate::procid::start_time(pid)
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        horch_platform::process::parent(pid).ok().flatten()
    }
}

/// The task and the attempt that a payload names.
trait OfAttempt {
    fn task(&self) -> &str;
    fn attempt(&self) -> &str;
}

macro_rules! of_attempt {
    ($($t:ty),*) => {$(
        impl OfAttempt for $t {
            fn task(&self) -> &str {
                &self.task
            }
            fn attempt(&self) -> &str {
                &self.attempt
            }
        }
    )*};
}

of_attempt!(HeartbeatFile, ExitFile, SessionFile);

/// Part 1: read the wrapper files of each of `attempts` under `fleet_root`.
/// `owner` is the user that owns the ledger; `heartbeat_max_bytes` is the
/// limit of `heartbeat.json` (`fleet.toml`). A path with an `ingested` row
/// is not read. Gives 1 read for each attempt.
pub fn read(
    conn: &Connection,
    fleet_root: &Path,
    attempts: &[AttemptRef],
    owner: &User,
    heartbeat_max_bytes: u64,
) -> Result<Vec<WrapperRead>> {
    let mut out = Vec::new();
    for a in attempts {
        let dir = attempt_dir(fleet_root, &a.task_id, a.attempt());
        let rule = |limit| Rule {
            root: &dir,
            owner,
            limit,
        };
        out.push(WrapperRead {
            attempt: a.clone(),
            launched: effects::read_launch(fleet_root, &a.attempt_id)?.is_some(),
            heartbeat: read_file(conn, &dir, HEARTBEAT_JSON, a, &rule(heartbeat_max_bytes))?,
            exit: read_file(conn, &dir, EXIT_JSON, a, &rule(EXIT_MAX_BYTES))?,
            session: read_file(conn, &dir, SESSION_JSON, a, &rule(SESSION_MAX_BYTES))?
                .map(checked_session),
        });
    }
    Ok(out)
}

/// The most bytes of the record id of `session.json`.
pub const RECORD_ID_MAX_BYTES: usize = 64;

/// `f` with its record id checked: the id is a claim of a file in the task
/// folder, so a form other than 1 to [`RECORD_ID_MAX_BYTES`] ASCII letters,
/// digits, `-` and `_` rejects the file.
fn checked_session(mut f: WrapperFile<SessionFile>) -> WrapperFile<SessionFile> {
    if let Found::Valid { sha256, file } = &f.found {
        if let Some(id) = file.record_id.as_deref().filter(|id| !record_id_ok(id)) {
            let shown: String = id.chars().take(RECORD_ID_MAX_BYTES).collect();
            f.found = Found::Rejected {
                sha256: Some(sha256.clone()),
                reason: format!(
                    "record_id {shown:?} is not 1 to {RECORD_ID_MAX_BYTES} ASCII letters, digits, - and _"
                ),
            };
        }
    }
    f
}

/// Whether `id` has the form of an execution record id.
fn record_id_ok(id: &str) -> bool {
    (1..=RECORD_ID_MAX_BYTES).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The `ingested.path` of a file.
fn key(path: &Path) -> String {
    path.display().to_string()
}

/// Read `dir/name` of attempt `a` under `rule`.
fn read_file<T: DeserializeOwned + OfAttempt>(
    conn: &Connection,
    dir: &Path,
    name: &str,
    a: &AttemptRef,
    rule: &Rule<'_>,
) -> Result<Option<WrapperFile<T>>> {
    let path = dir.join(name);
    if store::has(conn, &key(&path))? {
        return Ok(None);
    }
    if let Err(e) = std::fs::symlink_metadata(&path) {
        if e.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
    }
    let found = match read_protected(&path, rule) {
        Err(refusal) => Found::Rejected {
            sha256: None,
            reason: refusal.reason,
        },
        Ok(bytes) => {
            let sha256 = crate::digest::sha256_bytes(&bytes).hex();
            match payload::<T>(&bytes, a) {
                Ok(file) => Found::Valid { sha256, file },
                Err(reason) => Found::Rejected {
                    sha256: Some(sha256),
                    reason,
                },
            }
        }
    };
    Ok(Some(WrapperFile { path, found }))
}

/// The payload of `bytes`, which must name the task and the attempt of `a`.
fn payload<T: DeserializeOwned + OfAttempt>(bytes: &[u8], a: &AttemptRef) -> Result<T, String> {
    let file: T = serde_json::from_slice(bytes).map_err(|e| format!("schema: {e}"))?;
    if file.task() != a.task_id {
        return Err(format!(
            "task_id: the file names task {}, not {}",
            file.task(),
            a.task_id
        ));
    }
    if file.attempt() != a.attempt() {
        return Err(format!(
            "attempt_id: the file names attempt {}, not {}",
            file.attempt(),
            a.attempt()
        ));
    }
    Ok(file)
}

/// Part 2: apply each of `reads` in the open transaction `tx`, at fleet
/// clock `clock` and time `at`. The fleetd of generation `guard` writes.
/// `procs` gives the facts of the PID check.
pub fn ingest(
    tx: &Connection,
    guard: Option<Generation>,
    reads: &[WrapperRead],
    clock: FleetSeconds,
    at: &str,
    procs: &dyn ProcessFacts,
) -> Result<Report> {
    if tx.is_autocommit() {
        return Err(DoorError::NoTransaction.into());
    }
    if let Some(held) = guard {
        let found = door::generation(tx)?;
        if found != Some(held) {
            return Err(DoorError::GenerationLost { held, found }.into());
        }
    }
    let mut ingest = Ingest {
        tx,
        guard,
        clock,
        at,
        procs,
        report: Report::default(),
    };
    for r in reads {
        let id = &r.attempt.attempt_id;
        let worker: Option<String> = tx
            .query_row("SELECT worker_id FROM attempts WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(worker) = worker else {
            continue;
        };
        if let Some(f) = &r.heartbeat {
            ingest.heartbeat(id, &worker, r.launched, f)?;
        }
        // session.json first: it names the session record that the exit ends.
        if let Some(f) = &r.session {
            if let Some(session) = ingest.message(f)? {
                ingest.session(id, &worker, session)?;
            }
        }
        if let Some(f) = &r.exit {
            if let Some(exit) = ingest.message(f)? {
                ingest.exit(id, &worker, exit)?;
            }
        }
    }
    Ok(ingest.report)
}

/// 1 ingest in its transaction.
struct Ingest<'a> {
    tx: &'a Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    at: &'a str,
    procs: &'a dyn ProcessFacts,
    report: Report,
}

impl Ingest<'_> {
    /// Record the `ingested` row of `f`. Gives the payload of a valid file
    /// that had no row; `None` for a rejected file or a path with a row.
    fn message<'f, T>(&mut self, f: &'f WrapperFile<T>) -> Result<Option<&'f T>> {
        let (sha256, outcome, reason, file) = match &f.found {
            Found::Valid { sha256, file } => {
                (Some(sha256.as_str()), store::ACCEPTED, None, Some(file))
            }
            Found::Rejected { sha256, reason } => (
                sha256.as_deref(),
                store::REJECTED,
                Some(reason.as_str()),
                None,
            ),
        };
        let path = key(&f.path);
        if !store::record_message(self.tx, &path, sha256, outcome, reason, self.at)? {
            return Ok(None);
        }
        if file.is_none() {
            self.report.rejected.push(f.path.clone());
        }
        Ok(file)
    }

    /// Use the snapshot `f` of attempt `id` when its `seq` is new.
    /// `launched`: fleetd launched the attempt.
    fn heartbeat(
        &mut self,
        id: &str,
        worker: &str,
        launched: bool,
        f: &WrapperFile<HeartbeatFile>,
    ) -> Result<()> {
        let Found::Valid { file: hb, .. } = &f.found else {
            // A rejected snapshot is recorded like a message, and its path
            // is not read again.
            self.message(f)?;
            return Ok(());
        };
        let (last, state): (Option<i64>, String) = self.tx.query_row(
            "SELECT heartbeat_seq, state FROM attempts WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let Ok(seq) = i64::try_from(hb.seq) else {
            return Ok(());
        };
        if last.is_some_and(|last| seq <= last) {
            return Ok(());
        }
        let before = workers::get(self.tx, worker)?;
        self.tx.execute(
            "UPDATE attempts SET heartbeat_seq = ?2 WHERE id = ?1",
            params![id, seq],
        )?;
        self.tx.execute(
            "UPDATE workers SET last_checkin_at = ?2 WHERE worker_id = ?1",
            params![worker, self.at],
        )?;
        if let Some((agent, wrapper)) = agent_process(hb, &before, self.procs) {
            self.tx.execute(
                "UPDATE workers SET agent_pid = ?2, agent_started = ?3,
                    supervisor_pid = ?4, supervisor_started = ?5
                 WHERE worker_id = ?1",
                params![
                    worker,
                    i64::from(agent.0),
                    agent.1 as i64,
                    i64::from(wrapper.0),
                    wrapper.1 as i64
                ],
            )?;
            let recorded = Recorded {
                pid: wrapper.0,
                pid_start: Some(wrapper.1),
            };
            self.report.wrappers.insert(id.to_string(), recorded);
        } else if let Some(wrapper) = wrapper_process(hb, &before, self.procs)
            .filter(|_| launched && state != AttemptState::Ended.as_str())
        {
            self.tx.execute(
                "UPDATE workers SET supervisor_pid = ?2, supervisor_started = ?3
                 WHERE worker_id = ?1",
                params![worker, i64::from(wrapper.0), wrapper.1 as i64],
            )?;
            let recorded = Recorded {
                pid: wrapper.0,
                pid_start: Some(wrapper.1),
            };
            self.report.wrappers.insert(id.to_string(), recorded);
        }
        if output_since(hb, before.last_checkin_at.as_deref()) {
            leases_store::set_active_at(self.tx, id, self.clock.0)?;
        }
        self.report.checkins.push(id.to_string());
        Ok(())
    }

    /// Settle the worker of attempt `id` with the new `exit`, and end its
    /// session record. A push attempt keeps its worker row: the pass ends
    /// both by rung 5 in this pass, so an agent that ran never ends
    /// `launch_failed`, also when it exited before fleetd recorded its
    /// wrapper.
    fn exit(&mut self, id: &str, worker: &str, exit: &ExitFile) -> Result<()> {
        let ended = if self.pass_ends(id)? {
            supervisor::exit_seen(self.tx, worker, exit.code, self.at)?;
            self.report.exited.push(id.to_string());
            Ended::Clean
        } else {
            supervisor::settle_exit(self.tx, worker, exit.code, self.at)?
        };
        let noted = has_fact(self.tx, Entity::Attempt, id, EventKind::Note, REPAIR_CAUSE)?;
        if exit.repaired && !noted {
            let cause = Cause {
                actor: Actor::Supervisor,
                text: REPAIR_CAUSE,
            };
            let data = serde_json::json!({ "line": REPAIR_LINE });
            record(
                self.tx,
                self.guard,
                EventKind::Note,
                Entity::Attempt,
                id,
                cause,
                &data,
            )?;
        }
        if exit.outcome_hint == Some(OutcomeHint::Quota) {
            self.note_once(id, QUOTA_HINT)?;
        }
        self.report.exits.push((id.to_string(), ended));
        Ok(())
    }

    /// Whether attempt `id` is a push attempt that is not ended: the pass
    /// ends it by rung 5 at its `exit.json`.
    fn pass_ends(&self, id: &str) -> Result<bool> {
        let found: bool = self.tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM attempts WHERE id = ?1 AND via = 'push'
               AND state IN ('launching', 'live'))",
            [id],
            |r| r.get(0),
        )?;
        Ok(found)
    }

    /// Record the note `text` on attempt `id` 1 time.
    fn note_once(&self, id: &str, text: &str) -> Result<()> {
        if has_fact(self.tx, Entity::Attempt, id, EventKind::Note, text)? {
            return Ok(());
        }
        let cause = Cause {
            actor: Actor::Supervisor,
            text,
        };
        let data = serde_json::json!({});
        record(
            self.tx,
            self.guard,
            EventKind::Note,
            Entity::Attempt,
            id,
            cause,
            &data,
        )?;
        Ok(())
    }

    /// Fill the session of the worker of attempt `id` from the new
    /// session file `s`.
    fn session(&mut self, id: &str, worker: &str, s: &SessionFile) -> Result<()> {
        workers::set_session(
            self.tx,
            worker,
            Some(&s.session_id),
            s.transcript.as_deref(),
        )?;
        // `checked_session` checked the form of the id in part 1.
        if let Some(record_id) = &s.record_id {
            workers::set_record(self.tx, worker, record_id)?;
        }
        self.report.sessions.push(id.to_string());
        Ok(())
    }
}

/// The agent and its wrapper, each as (PID, start time), that heartbeat
/// `hb` names, when they pass the PID check against the worker row `w`.
fn agent_process(
    hb: &HeartbeatFile,
    w: &workers::Worker,
    procs: &dyn ProcessFacts,
) -> Option<((u32, u64), (u32, u64))> {
    let fleetd = std::process::id();
    let pid = hb.pid.filter(|&p| p > 1 && p != fleetd)?;
    let start = procs.start_time(pid)?;
    let wrapper = procs.parent(pid).filter(|&p| p > 1 && p != fleetd)?;
    let wrapper_start = procs.start_time(wrapper)?;
    let recorded = w
        .supervisor_pid
        .and_then(|p| u32::try_from(p).ok())
        .zip(w.supervisor_started.and_then(|s| u64::try_from(s).ok()));
    if let Some(recorded) = recorded {
        let runs = procs.start_time(recorded.0) == Some(recorded.1);
        if runs && recorded != (wrapper, wrapper_start) {
            return None;
        }
    }
    Some(((pid, start), (wrapper, wrapper_start)))
}

/// The wrapper, as (PID, start time), of a heartbeat `hb` that names no
/// agent: it runs, it is not fleetd, and the worker row `w` has no wrapper
/// or has this one. A wrapper that the row has is never replaced here.
fn wrapper_process(
    hb: &HeartbeatFile,
    w: &workers::Worker,
    procs: &dyn ProcessFacts,
) -> Option<(u32, u64)> {
    if hb.pid.is_some() {
        return None;
    }
    let fleetd = std::process::id();
    let pid = hb.wrapper.filter(|&p| p > 1 && p != fleetd)?;
    let start = procs.start_time(pid)?;
    let recorded = (w.supervisor_pid, w.supervisor_started);
    match recorded {
        (None, None) => Some((pid, start)),
        (Some(p), Some(s)) if i64::from(pid) == p && start as i64 == s => Some((pid, start)),
        _ => None,
    }
}

/// Whether the agent of `hb` wrote output after the checkin `last`.
fn output_since(hb: &HeartbeatFile, last: Option<&str>) -> bool {
    let Some(output) = hb.last_output_at.as_deref() else {
        return false;
    };
    match last {
        None => true,
        Some(last) => util::seconds_between(last, output).is_some_and(|s| s > 0),
    }
}
