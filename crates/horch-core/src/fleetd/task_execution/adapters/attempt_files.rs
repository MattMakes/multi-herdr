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

pub const QUOTA_HINT: &str = "the wrapper saw a usage limit";

pub const EXIT_MAX_BYTES: u64 = 4096;

pub const SESSION_MAX_BYTES: u64 = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found<T> {
    Valid {
        sha256: String,
        file: T,
    },
    Rejected {
        sha256: Option<String>,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrapperFile<T> {
    pub path: PathBuf,
    pub found: Found<T>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrapperRead {
    pub attempt: AttemptRef,
    pub launched: bool,
    pub heartbeat: Option<WrapperFile<HeartbeatFile>>,
    pub exit: Option<WrapperFile<ExitFile>>,
    pub session: Option<WrapperFile<SessionFile>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub checkins: Vec<String>,
    pub exited: Vec<String>,
    pub wrappers: BTreeMap<String, Recorded>,
    pub exits: Vec<(String, Ended)>,
    pub sessions: Vec<String>,
    pub rejected: Vec<PathBuf>,
}

pub trait ProcessFacts {
    fn start_time(&self, pid: u32) -> Option<u64>;
    fn parent(&self, pid: u32) -> Option<u32>;
}

pub struct OsProcesses;

impl ProcessFacts for OsProcesses {
    fn start_time(&self, pid: u32) -> Option<u64> {
        crate::procid::start_time(pid)
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        horch_platform::process::parent(pid).ok().flatten()
    }
}

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

pub const RECORD_ID_MAX_BYTES: usize = 64;

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

fn record_id_ok(id: &str) -> bool {
    (1..=RECORD_ID_MAX_BYTES).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn key(path: &Path) -> String {
    path.display().to_string()
}

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

struct Ingest<'a> {
    tx: &'a Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    at: &'a str,
    procs: &'a dyn ProcessFacts,
    report: Report,
}

impl Ingest<'_> {
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

    fn heartbeat(
        &mut self,
        id: &str,
        worker: &str,
        launched: bool,
        f: &WrapperFile<HeartbeatFile>,
    ) -> Result<()> {
        let Found::Valid { file: hb, .. } = &f.found else {
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

    fn pass_ends(&self, id: &str) -> Result<bool> {
        let found: bool = self.tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM attempts WHERE id = ?1 AND via = 'push'
               AND state IN ('launching', 'live'))",
            [id],
            |r| r.get(0),
        )?;
        Ok(found)
    }

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

    fn session(&mut self, id: &str, worker: &str, s: &SessionFile) -> Result<()> {
        workers::set_session(
            self.tx,
            worker,
            Some(&s.session_id),
            s.transcript.as_deref(),
        )?;
        if let Some(record_id) = &s.record_id {
            workers::set_record(self.tx, worker, record_id)?;
        }
        self.report.sessions.push(id.to_string());
        Ok(())
    }
}

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

fn output_since(hb: &HeartbeatFile, last: Option<&str>) -> bool {
    let Some(output) = hb.last_output_at.as_deref() else {
        return false;
    };
    match last {
        None => true,
        Some(last) => util::seconds_between(last, output).is_some_and(|s| s > 0),
    }
}
