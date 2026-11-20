//! The execution store: a project's session records.
//!
//! The records live in the `session_records` table of a task ledger file
//! (TRF-45, D10), 1 row per [`LedgerRecordV1`], the record as JSON. Only
//! fleetd writes the ledger (RR-5): a read-modify-write reads the rows
//! read-only and sends the changed rows to fleetd in 1 `session_records`
//! request (T-884). The wrapper of a worker sends no request (T-899): its
//! changed rows stay in its process, and fleetd gets its session from
//! `session.json` of its attempt. The old JSON ledger
//! at `<state_root>/<slug>.json` (the path and slug rule of every earlier
//! version) is read, never written: `horch ledger import` copies it, and a
//! record only it holds moves into the table at the next write.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{bail, ensure, Context, Result};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use chrono::{DateTime, Utc};

use crate::execution::legacy::{
    HistoryEntry, LedgerRecordV1, LedgerWindow, KIND_ORCHESTRATOR, KIND_WORKER, STATUS_WORKING,
};
use crate::execution::model::{
    Execution, ExecutionKind, ExecutionStatus, FailureKind, LaunchStage, SessionState, Task,
};
use crate::fleetd::fleet_runtime::adapters::channel_client::{self, Sent};
use crate::fleetd::fleet_runtime::adapters::request_wire::Outcome;
use crate::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use crate::ids::{IdError, PaneId, WorkspaceId};
use crate::runtime::{Paths, RuntimeContext};
use crate::skills::activation::ResolvedSkillRef;
use crate::taskledger::store::LedgerWriter;
use crate::workspace::client::WorkspaceClient;

/// The history text of a record that [`ExecutionStore::end_if_pane_closed`]
/// ended.
pub const PANE_CLOSED: &str = "pane closed without horch done";

/// Turn a project path into a filename, matching `tr -c 'A-Za-z0-9' '-'`.
///
/// Operates on bytes, exactly as `tr` does, so a multi-byte character becomes one
/// `-` per byte and slugs computed by the bash version still resolve.
pub fn slug(project: &str) -> String {
    project
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect()
}

/// Every ledger file directly under `state_root`, sorted: each `*.json` file
/// except `policy.json`. Directories are never ledgers, so the dataset tree
/// under `<state_root>/multi-herdr/` is never read (OD3).
pub(crate) fn ledger_paths(state_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(state_root) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension().and_then(|e| e.to_str()) == Some("json")
                && p.file_name().and_then(|n| n.to_str()) != Some("policy.json")
        })
        .collect();
    paths.sort();
    paths
}

/// One ledger file's records, or `None` when it cannot be read or does not
/// parse right now (a write by an old binary may be in flight). An empty
/// file is an empty ledger.
pub(crate) fn read_ledger_file(path: &Path) -> Option<Vec<LedgerRecordV1>> {
    let text = std::fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        return Some(Vec::new());
    }
    serde_json::from_str(&text).ok()
}

/// Every ledger under `state_root` that reads right now, with its path, in
/// path order. A ledger that does not parse is left out; a reader that runs
/// across ticks keeps its last good copy itself (the telemetry collector).
pub fn read_all_ledgers(state_root: &Path) -> Vec<(PathBuf, Vec<LedgerRecordV1>)> {
    ledger_paths(state_root)
        .into_iter()
        .filter_map(|p| read_ledger_file(&p).map(|records| (p, records)))
        .collect()
}

/// The `label` prefix that marks a judge record; the attempt follows it.
/// A judge has no key of its own: `round_id` plus `judge:<attempt>` is its
/// key (architecture design §4.3).
const JUDGE_LABEL: &str = "judge:";

/// How long a `Planned` record may wait for its pane before a later spawn
/// treats its spawner as gone. A spawn takes seconds from insert to pane.
pub const ABANDONED_AFTER: chrono::Duration = chrono::Duration::minutes(5);

/// Why a ledger record does not convert to an [`Execution`].
#[derive(Debug)]
pub enum LegacyError {
    Id(IdError),
    Agent(String),
    /// `kind`, `experiment_id`, `round_id` and `label` do not name a kind.
    Kind {
        record: String,
        reason: String,
    },
}

impl std::fmt::Display for LegacyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id(e) => write!(f, "{e}"),
            Self::Agent(e) => f.write_str(e),
            Self::Kind { record, reason } => write!(f, "record {record}: {reason}"),
        }
    }
}

impl std::error::Error for LegacyError {}

impl From<IdError> for LegacyError {
    fn from(e: IdError) -> Self {
        Self::Id(e)
    }
}

fn kind_error(r: &LedgerRecordV1, reason: &str) -> LegacyError {
    LegacyError::Kind {
        record: r.record_id.clone(),
        reason: reason.to_string(),
    }
}

fn kind_of(r: &LedgerRecordV1) -> Result<ExecutionKind, LegacyError> {
    match r.kind.as_str() {
        KIND_ORCHESTRATOR => orchestrator_kind(r),
        KIND_WORKER => worker_kind(r),
        other => Err(kind_error(r, &format!("unknown kind '{other}'"))),
    }
}

fn orchestrator_kind(r: &LedgerRecordV1) -> Result<ExecutionKind, LegacyError> {
    match (&r.experiment_id, &r.round_id, &r.label) {
        (None, None, None) => Ok(ExecutionKind::Orchestrator),
        _ => Err(kind_error(
            r,
            "an orchestrator has no experiment, round or label",
        )),
    }
}

fn worker_kind(r: &LedgerRecordV1) -> Result<ExecutionKind, LegacyError> {
    match (&r.experiment_id, &r.round_id, &r.label) {
        (None, None, None) => Ok(ExecutionKind::Worker),
        (None, Some(round), Some(label)) if label.starts_with(JUDGE_LABEL) => {
            judge_kind(r, round, label)
        }
        (Some(experiment), Some(round), Some(label)) => Ok(ExecutionKind::Candidate {
            experiment: experiment.parse()?,
            round: round.parse()?,
            label: label.clone(),
        }),
        _ => Err(kind_error(
            r,
            "experiment_id, round_id and label do not name a kind",
        )),
    }
}

fn judge_kind(r: &LedgerRecordV1, round: &str, label: &str) -> Result<ExecutionKind, LegacyError> {
    let attempt = label[JUDGE_LABEL.len()..]
        .parse()
        .map_err(|_| kind_error(r, &format!("judge label '{label}' has no attempt number")))?;
    Ok(ExecutionKind::Judge {
        round: round.parse()?,
        attempt,
    })
}

/// The typed view of a ledger record. Every key of the record has a place in
/// [`Execution`], so [`from_execution`] gives the record back unchanged.
///
/// A `session_id` of `""` (the bash spelling for none) reads as
/// [`SessionState::Unavailable`] and writes back as `""`.
pub fn to_execution(r: &LedgerRecordV1) -> Result<Execution, LegacyError> {
    let (workspace, pane) = placement(r)?;
    Ok(Execution {
        id: r.record_id.parse()?,
        kind: kind_of(r)?,
        teammate: r.tier.parse()?,
        harness: r.agent.parse().map_err(LegacyError::Agent)?,
        model: r.model.clone(),
        model_override: r.model_override,
        effort: r.effort.clone(),
        phase: r.phase,
        role: r.role.parse()?,
        task: task_of(r)?,
        status: r.execution_status(),
        typed_status: r.state.is_some(),
        session: session_state(r.session_id.as_deref())?,
        exit_code: r.exit_code,
        project: r.project.as_ref().map(std::path::PathBuf::from),
        workdir: r.workdir.as_ref().map(std::path::PathBuf::from),
        workspace,
        pane,
        skills: r.skills.clone(),
        routing: r.routing.clone(),
        via: r.via.clone(),
        substitution_reason: r.substitution_reason.clone(),
        history: r.history.clone(),
        created_at: r.created_at.clone(),
        updated_at: r.updated_at.clone(),
        finished_at: r.finished_at.clone(),
    })
}

fn session_state(id: Option<&str>) -> Result<SessionState, LegacyError> {
    match id {
        None => Ok(SessionState::Pending),
        Some("") => Ok(SessionState::Unavailable),
        Some(id) => Ok(SessionState::Known(id.parse()?)),
    }
}

fn placement(r: &LedgerRecordV1) -> Result<(Option<WorkspaceId>, Option<PaneId>), LegacyError> {
    Ok((
        r.workspace_id.as_deref().map(str::parse).transpose()?,
        r.pane_id.as_deref().map(str::parse).transpose()?,
    ))
}

fn task_of(r: &LedgerRecordV1) -> Result<Task, LegacyError> {
    Ok(Task {
        id: r.task_id.as_deref().map(str::parse).transpose()?,
        text: r.task.clone(),
        plan: r.plan.clone(),
    })
}

/// The ledger record of an execution: the inverse of [`to_execution`].
pub fn from_execution(e: &Execution) -> LedgerRecordV1 {
    let text =
        |p: &Option<std::path::PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned());
    let (experiment_id, round_id, label) = match &e.kind {
        ExecutionKind::Worker | ExecutionKind::Orchestrator => (None, None, None),
        ExecutionKind::Candidate {
            experiment,
            round,
            label,
        } => (
            Some(experiment.to_string()),
            Some(round.to_string()),
            Some(label.clone()),
        ),
        ExecutionKind::Judge { round, attempt } => (
            None,
            Some(round.to_string()),
            Some(format!("{JUDGE_LABEL}{attempt}")),
        ),
    };
    LedgerRecordV1 {
        record_id: e.id.to_string(),
        session_id: match &e.session {
            SessionState::Pending => None,
            SessionState::Unavailable => Some(String::new()),
            SessionState::Known(id) => Some(id.to_string()),
        },
        agent: e.harness.as_str().to_string(),
        tier: e.teammate.to_string(),
        model: e.model.clone(),
        effort: e.effort.clone(),
        phase: e.phase,
        role: e.role.to_string(),
        status: e.status.legacy_status().to_string(),
        task: e.task.text.clone(),
        history: e.history.clone(),
        created_at: e.created_at.clone(),
        updated_at: e.updated_at.clone(),
        kind: e.kind.legacy_kind().to_string(),
        project: text(&e.project),
        plan: e.task.plan.clone(),
        workspace_id: e.workspace.as_ref().map(|w| w.to_string()),
        via: e.via.clone(),
        substitution_reason: e.substitution_reason.clone(),
        routing: e.routing.clone(),
        pane_id: e.pane.as_ref().map(|p| p.to_string()),
        task_id: e.task.id.as_ref().map(|t| t.to_string()),
        workdir: text(&e.workdir),
        experiment_id,
        round_id,
        label,
        state: e.typed_status.then(|| e.status.clone()),
        exit_code: e.exit_code,
        finished_at: e.finished_at.clone(),
        skills: e.skills.clone(),
        compact_window: None,
        model_override: e.model_override,
    }
}

/// The file name of the session records under a state root that has no
/// task ledger of its own: a test, or a state dir moved with
/// `HORCH_STATE_DIR` alone.
pub const STATE_DB: &str = "ledger.sqlite3";

/// The task ledger file that holds the session records of `paths`
/// (TRF-45, T-890). An explicit value wins, in this order:
/// 1. An explicit `HORCH_LEDGER_DB` gives that ledger. Only fleetd writes
///    the session rows (T-884), so they must be in the ledger of that
///    fleetd, and the fleetd launches each process with its ledger.
/// 2. With no explicit ledger, a state dir moved with `HORCH_STATE_DIR` (a
///    test, a smoke run) gives `<state_root>/ledger.sqlite3`, so it never
///    writes the sessions of the operator's ledger.
/// 3. Else the task ledger. A worker pane gets the default state dir by
///    name, which is no move.
pub fn session_db(paths: &Paths) -> PathBuf {
    if paths.ledger_override.is_some() || paths.state_root == paths.default_state_root {
        paths.ledger_db.clone()
    } else {
        paths.state_root.join(STATE_DB)
    }
}

/// The fleet root of the fleetd that owns the session records file `db`,
/// for a process with `paths`: the fleet root of `paths` when `db` is its
/// ledger, else the folder `fleet` beside `db`.
pub fn session_fleet_root(paths: &Paths, db: &Path) -> PathBuf {
    if paths.ledger_db == db {
        paths.fleet_root.clone()
    } else {
        db.parent().unwrap_or(Path::new("")).join("fleet")
    }
}

/// The session records file and the task ledger to overlay for a reader that
/// knows only `state_root`: this process's own files when `state_root` is
/// this process's state root, else `<state_root>/ledger.sqlite3` and no
/// overlay.
pub fn session_files(state_root: &Path) -> (PathBuf, Option<PathBuf>) {
    let paths = Paths::from_env(&crate::runtime::ProcessEnv);
    if paths.state_root == state_root {
        (session_db(&paths), Some(paths.ledger_db))
    } else {
        (state_root.join(STATE_DB), None)
    }
}

/// The rule of [`ExecutionStore::record_exit`] for the record `r`, at
/// `at`. True when it ended the record.
fn exit_record(r: &mut LedgerRecordV1, code: Option<i32>, at: &str) -> bool {
    r.exit_code = code;
    r.updated_at = at.to_string();
    if r.is_orchestrator() || !r.execution_status().is_live() {
        return false;
    }
    let state = match code {
        Some(0) => ExecutionStatus::Done,
        code => ExecutionStatus::Failed {
            failure: FailureKind::AgentExited { code },
        },
    };
    r.finished_at.get_or_insert_with(|| at.to_string());
    r.set_state(state);
    true
}

/// End the session record `record_id` in the open transaction `tx` of the
/// ledger file that holds it, by the rule of
/// [`ExecutionStore::record_exit`], at `at`. The ingest of `exit.json`
/// uses it (fleetd-p5 ticket 43). A record that this file does not hold,
/// or that does not parse, changes nothing. A record that already holds
/// the end and `code` changes nothing, so a repeat is a no-op. True when
/// it ended the record.
pub fn record_exit_in(
    tx: &rusqlite::Connection,
    record_id: &str,
    code: Option<i32>,
    at: &str,
) -> Result<bool> {
    let json: Option<String> = tx
        .query_row(
            "SELECT record_json FROM session_records WHERE record_id = ?1",
            [record_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(mut r) = json.and_then(|j| serde_json::from_str::<LedgerRecordV1>(&j).ok()) else {
        return Ok(false);
    };
    let live = !r.is_orchestrator() && r.execution_status().is_live();
    if !live && r.exit_code == code {
        return Ok(false);
    }
    let ended = exit_record(&mut r, code, at);
    r.sync_legacy_status();
    tx.execute(
        "UPDATE session_records SET record_json = ?2, updated_at = ?3 WHERE record_id = ?1",
        rusqlite::params![record_id, serde_json::to_string(&r)?, r.updated_at],
    )?;
    Ok(ended)
}

/// The reason of a `session_records` request that fleetd refuses because a
/// row changed after its client read it. The client reads again and sends
/// again.
pub const SESSION_RECORD_CHANGED: &str = "a session record changed after it was read";

/// The error of a `session_records` row that changed after its client read
/// it ([`SESSION_RECORD_CHANGED`]).
#[derive(Debug)]
pub struct SessionRecordChanged {
    pub record_id: String,
}

impl std::fmt::Display for SessionRecordChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{SESSION_RECORD_CHANGED}: {}", self.record_id)
    }
}

impl std::error::Error for SessionRecordChanged {}

/// The most bytes of a project path in a `session_records` request.
const PROJECT_ROOT_MAX_BYTES: usize = 4096;

/// The most bytes of a record id in a `session_records` request.
const RECORD_ID_MAX_BYTES: usize = 64;

/// The payload of the request kind `session_records` (T-884): the session
/// records of 1 project that 1 [`ExecutionStore::update`] changed. fleetd
/// is the only writer of the ledger (RR-5), so each process sends its
/// change, and fleetd applies it with [`apply_session_records`].
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRecordsPayload {
    /// The project path as the store has it. Its slug names the rows.
    pub project_root: String,
    pub rows: Vec<SessionRowChange>,
}

/// 1 changed session record of a [`SessionRecordsPayload`].
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRowChange {
    pub record_id: String,
    /// The sha256 of the stored `record_json` that the client read. `None`:
    /// the client read no row.
    #[serde(default)]
    pub expected: Option<String>,
    /// The new record. `None` removes the row.
    #[serde(default)]
    pub record: Option<LedgerRecordV1>,
}

/// The sha256 of a stored `record_json`, as hex.
fn row_hash(json: &str) -> String {
    crate::digest::sha256_bytes(json.as_bytes()).hex()
}

/// Whether `id` has the form of a record id: 1 to 64 ASCII letters,
/// digits, `-` and `_`.
fn record_id_ok(id: &str) -> bool {
    (1..=RECORD_ID_MAX_BYTES).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Check the form of the untrusted payload `p`: a project path, at least 1
/// row, each record id 1 time and of the record id form, each record with
/// the id of its row, and each `expected` a sha256 in hex.
fn check_session_records(p: &SessionRecordsPayload) -> Result<()> {
    ensure!(
        !p.project_root.trim().is_empty() && p.project_root.len() <= PROJECT_ROOT_MAX_BYTES,
        "the project_root of session_records is empty or longer than {PROJECT_ROOT_MAX_BYTES} bytes"
    );
    ensure!(!p.rows.is_empty(), "session_records has no row");
    let mut seen = std::collections::HashSet::new();
    for row in &p.rows {
        let shown: String = row.record_id.chars().take(RECORD_ID_MAX_BYTES).collect();
        ensure!(
            record_id_ok(&row.record_id),
            "the record_id {shown:?} is not 1 to {RECORD_ID_MAX_BYTES} ASCII letters, digits, - and _"
        );
        ensure!(
            seen.insert(row.record_id.as_str()),
            "session_records names the record {shown} 2 times"
        );
        if let Some(r) = &row.record {
            ensure!(
                r.record_id == row.record_id,
                "the record of the row {shown} has another record_id"
            );
        }
        if let Some(hash) = &row.expected {
            ensure!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "the expected hash of the row {shown} is not a sha256 in hex"
            );
        }
    }
    Ok(())
}

/// Apply 1 `session_records` request in the open transaction `tx` of the
/// ledger: the handler of fleetd. The payload is untrusted, so its form is
/// checked first. Each row applies only when its stored `record_json` is the
/// one that the client read, else the request is refused with
/// [`SESSION_RECORD_CHANGED`] and writes nothing. A row of another project
/// is refused. A changed row keeps its place in the write order, and a new
/// row goes last.
pub fn apply_session_records(tx: &rusqlite::Connection, payload: &Value) -> Result<Value> {
    let p: SessionRecordsPayload = serde_json::from_value(payload.clone())
        .context("the payload of session_records does not fit its kind")?;
    check_session_records(&p)?;
    let slug = slug(&p.project_root);
    for row in &p.rows {
        let stored: Option<(String, String)> = tx
            .query_row(
                "SELECT slug, record_json FROM session_records WHERE record_id = ?1",
                [&row.record_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((other, _)) = stored.as_ref().filter(|(s, _)| *s != slug) {
            bail!(
                "the session record {} is a record of the project slug {other}",
                row.record_id
            );
        }
        let found = stored.as_ref().map(|(_, json)| row_hash(json));
        if found != row.expected {
            return Err(SessionRecordChanged {
                record_id: row.record_id.clone(),
            }
            .into());
        }
        put_row(tx, &p.project_root, &slug, row, stored.is_some())?;
    }
    Ok(serde_json::json!({ "rows": p.rows.len() }))
}

/// Write the row change `row` of `project` in `tx`. `stored`: the row
/// exists. A record's `status` is brought in line with its `state` first.
fn put_row(
    tx: &rusqlite::Connection,
    project: &str,
    slug: &str,
    row: &SessionRowChange,
    stored: bool,
) -> Result<()> {
    let Some(record) = &row.record else {
        tx.execute(
            "DELETE FROM session_records WHERE record_id = ?1",
            [&row.record_id],
        )?;
        return Ok(());
    };
    let mut r = record.clone();
    r.sync_legacy_status();
    let json = serde_json::to_string(&r)?;
    let sql = if stored {
        "UPDATE session_records SET
             worker_id = (SELECT worker_id FROM workers
                          WHERE worker_id = ?1 OR legacy_record_id = ?1
                          ORDER BY rowid DESC LIMIT 1),
             project_root = ?2, slug = ?3, record_json = ?4, updated_at = ?5
         WHERE record_id = ?1"
    } else {
        "INSERT INTO session_records
             (record_id, worker_id, project_root, slug, record_json, updated_at)
         VALUES (?1, (SELECT worker_id FROM workers
                      WHERE worker_id = ?1 OR legacy_record_id = ?1
                      ORDER BY rowid DESC LIMIT 1),
                 ?2, ?3, ?4, ?5)"
    };
    tx.execute(
        sql,
        rusqlite::params![r.record_id, project, slug, json, r.updated_at],
    )?;
    Ok(())
}

/// What became of the change of 1 [`ExecutionStore::update_written`].
#[derive(Debug, PartialEq)]
pub enum SessionWrite {
    /// The change is in the ledger, or no row changed. The wrapper of a
    /// worker keeps its change in its process.
    Committed,
    /// fleetd is down: the change waits in the request file `path` and is
    /// not in the ledger.
    Queued {
        request_id: RequestId,
        path: PathBuf,
    },
}

/// The most reads of 1 [`ExecutionStore::update`] whose change fleetd
/// refuses with [`SESSION_RECORD_CHANGED`].
const UPDATE_TRIES: usize = 3;

/// The changes from the stored rows `stored` (record id to `record_json`)
/// to `records`: each new or changed record, then each stored record that
/// `records` dropped. A record id that appears twice keeps its last record.
/// A record of the old JSON ledger only is new, so it moves into the table.
fn row_changes(
    stored: &HashMap<String, String>,
    records: &[LedgerRecordV1],
) -> Result<Vec<SessionRowChange>> {
    let mut last: HashMap<&str, usize> = HashMap::new();
    for (i, r) in records.iter().enumerate() {
        last.insert(r.record_id.as_str(), i);
    }
    let mut out = Vec::new();
    for (i, r) in records.iter().enumerate() {
        if last.get(r.record_id.as_str()) != Some(&i) {
            continue;
        }
        let mut r = r.clone();
        r.sync_legacy_status();
        let before = stored.get(&r.record_id);
        let json = serde_json::to_string(&r)?;
        if before.is_some_and(|stored| *stored == json) {
            continue;
        }
        out.push(SessionRowChange {
            record_id: r.record_id.clone(),
            expected: before.map(|json| row_hash(json)),
            record: Some(r),
        });
    }
    let mut dropped: Vec<(&String, &String)> = stored
        .iter()
        .filter(|(id, _)| !last.contains_key(id.as_str()))
        .collect();
    dropped.sort();
    out.extend(dropped.into_iter().map(|(id, json)| SessionRowChange {
        record_id: id.clone(),
        expected: Some(row_hash(json)),
        record: None,
    }));
    Ok(out)
}

/// 1 row of `session_records`.
struct SessionRow {
    project_root: String,
    slug: String,
    record: LedgerRecordV1,
    /// The stored `record_json`, for the hash of a change.
    json: String,
}

/// The session records of the slug `slug` (every slug with `None`), in write
/// order. A record that does not parse is left out.
fn session_rows(conn: &rusqlite::Connection, slug: Option<&str>) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT project_root, slug, record_json FROM session_records
         WHERE ?1 IS NULL OR slug = ?1 ORDER BY rowid",
    )?;
    let rows = stmt
        .query_map([slug], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(project_root, slug, json)| {
            let record = serde_json::from_str(&json).ok()?;
            Some(SessionRow {
                project_root,
                slug,
                record,
                json,
            })
        })
        .collect())
}

fn has_session_records_table(conn: &rusqlite::Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'session_records')",
        [],
        |row| row.get(0),
    )?)
}

/// The session records in `db` when the file exists, without creating it.
/// A ledger that does not open or read gives none: a read never fails on it.
fn read_session_rows(db: &Path, slug: Option<&str>) -> Vec<SessionRow> {
    if !db.is_file() {
        return Vec::new();
    }
    crate::taskledger::Store::open_read_only(db)
        .and_then(|store| session_rows(store.conn(), slug))
        .unwrap_or_default()
}

/// The old JSON records (read only) that no session record replaces, then
/// the session records: 1 project's records in write order.
fn merge_legacy(legacy: Vec<LedgerRecordV1>, rows: Vec<LedgerRecordV1>) -> Vec<LedgerRecordV1> {
    let stored: std::collections::HashSet<&str> =
        rows.iter().map(|r| r.record_id.as_str()).collect();
    let mut out: Vec<LedgerRecordV1> = legacy
        .into_iter()
        .filter(|r| !stored.contains(r.record_id.as_str()))
        .collect();
    out.extend(rows);
    out
}

/// 1 project's records, as [`read_all_projects`] reads them.
pub struct ProjectRecords {
    /// The path its JSON ledger had: `<state_root>/<slug>.json`.
    pub path: PathBuf,
    /// The project path its session records or JSON records name.
    pub project: Option<String>,
    pub records: Vec<LedgerRecordV1>,
}

/// Every project's records under `state_root`, in path order: the old JSON
/// ledgers (read only) merged with the session records in `db`, then the
/// worker rows of `task_ledger` laid over each project. A JSON ledger that
/// does not parse right now is left out.
pub fn read_all_projects(
    state_root: &Path,
    db: &Path,
    task_ledger: Option<&Path>,
) -> Vec<ProjectRecords> {
    merge_projects(state_root, read_all_ledgers(state_root), db, task_ledger)
}

/// [`read_all_projects`] over JSON ledgers the caller read already
/// (`legacy`, path and records): a reader that keeps its last good copy of
/// each file (the telemetry collector) passes its copies.
pub fn merge_projects(
    state_root: &Path,
    legacy: Vec<(PathBuf, Vec<LedgerRecordV1>)>,
    db: &Path,
    task_ledger: Option<&Path>,
) -> Vec<ProjectRecords> {
    let mut by_path: std::collections::BTreeMap<PathBuf, (Option<String>, Vec<LedgerRecordV1>)> =
        legacy
            .into_iter()
            .map(|(p, records)| (p, (None, records)))
            .collect();
    let mut rows: std::collections::BTreeMap<PathBuf, (String, Vec<LedgerRecordV1>)> =
        Default::default();
    for row in read_session_rows(db, None) {
        let path = state_root.join(format!("{}.json", row.slug));
        rows.entry(path)
            .or_insert_with(|| (row.project_root.clone(), Vec::new()))
            .1
            .push(row.record);
    }
    for (path, (project, records)) in rows {
        let entry = by_path.entry(path).or_default();
        entry.0 = Some(project);
        entry.1 = merge_legacy(std::mem::take(&mut entry.1), records);
    }
    by_path
        .into_iter()
        .map(|(path, (project, mut records))| {
            let project = project.or_else(|| records.iter().find_map(|r| r.project.clone()));
            if let Some((db, project)) = task_ledger.zip(project.as_deref()) {
                overlay_task_ledger(&mut records, db, Path::new(project));
            }
            ProjectRecords {
                path,
                project,
                records,
            }
        })
        .collect()
}

/// [`read_all_projects`] as (path, records) pairs.
pub fn read_all_sessions(
    state_root: &Path,
    db: &Path,
    task_ledger: Option<&Path>,
) -> Vec<(PathBuf, Vec<LedgerRecordV1>)> {
    read_all_projects(state_root, db, task_ledger)
        .into_iter()
        .map(|p| (p.path, p.records))
        .collect()
}

struct WrapperRow {
    db: PathBuf,
    slug: String,
    record: LedgerRecordV1,
}

static WRAPPER_ROWS: Mutex<Vec<WrapperRow>> = Mutex::new(Vec::new());

/// A project's executions.
///
/// The records live in the `session_records` table of a task ledger file
/// (TRF-45, D10): 1 row per record, the record as JSON. An old JSON ledger
/// (`<state_root>/<slug>.json`) is read, never written: a record only it
/// holds reads as before and moves into the table at the next write. Reads
/// lay the task ledger's worker rows over the records (TRF-44,
/// [`overlay_task_ledger`]). Every write is 1 `session_records` request to
/// fleetd ([`ExecutionStore::update`]).
#[derive(Debug, Clone)]
pub struct ExecutionStore {
    /// The old JSON ledger, read only.
    path: PathBuf,
    /// The task ledger file that holds the session records.
    db: PathBuf,
    /// The project path as given, and its slug.
    project: String,
    slug: String,
    /// The task ledger and the project root its runs are under.
    task_ledger: Option<(PathBuf, PathBuf)>,
    /// The context that reaches the fleetd of [`Self::db`], when the store
    /// was opened from 1 ([`ExecutionStore::open_in`]).
    fleet: Option<Box<RuntimeContext>>,
    /// The paths the store was opened with ([`ExecutionStore::open`]).
    paths: Option<Box<Paths>>,
    /// The writer of fleetd: with it, a write applies in this process, on
    /// the path of the `session_records` handler.
    writer: Option<Arc<LedgerWriter>>,
}

impl ExecutionStore {
    /// The store for an explicit state root and project path. The session
    /// records live in `<state_root>/ledger.sqlite3`.
    pub fn for_project(state_root: impl AsRef<Path>, project: &str) -> Self {
        let state_root = state_root.as_ref();
        Self::in_db(state_root, &state_root.join(STATE_DB), project)
    }

    /// The store for `project` with its session records in `db` and its old
    /// JSON ledger under `state_root`.
    pub fn in_db(state_root: &Path, db: &Path, project: &str) -> Self {
        let slug = slug(project);
        Self {
            path: state_root.join(format!("{slug}.json")),
            db: db.to_path_buf(),
            project: project.to_string(),
            slug,
            task_ledger: None,
            fleet: None,
            paths: None,
            writer: None,
        }
    }

    /// The store for `project` under the state root of `paths`, with its
    /// session records in [`session_db`], reading through the task ledger of
    /// `paths`.
    pub fn open(paths: &Paths, project: &Path) -> Self {
        let mut store = Self::in_db(
            &paths.state_root,
            &session_db(paths),
            &project.to_string_lossy(),
        )
        .with_task_ledger(&paths.ledger_db, project);
        store.paths = Some(Box::new(paths.clone()));
        store
    }

    /// The store for the context's state root and project, reading through
    /// the context's task ledger.
    pub fn open_in(ctx: &RuntimeContext) -> Result<Self> {
        let mut store = Self::open(&ctx.paths, &ctx.paths.project()?);
        store.fleet = Some(Box::new(ctx.clone()));
        Ok(store)
    }

    /// Write through the writer of fleetd, in this process, with the
    /// handler of `session_records` ([`apply_session_records`]). Only a
    /// process that holds the writer (fleetd, or a test of the store) can
    /// call it.
    pub fn with_writer(mut self, writer: Arc<LedgerWriter>) -> Self {
        self.writer = Some(writer);
        self
    }

    /// Read the worker rows of `project`'s runs in the task ledger `db` over
    /// the records. A ledger file that does not exist adds nothing.
    pub fn with_task_ledger(mut self, db: &Path, project: &Path) -> Self {
        self.task_ledger = Some((db.to_path_buf(), project.to_path_buf()));
        self
    }

    /// The old JSON ledger of this project. Nothing writes it (TRF-45).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The task ledger file that holds this project's session records.
    pub fn db(&self) -> &Path {
        &self.db
    }

    /// All records, oldest write order preserved, with the task ledger's
    /// worker rows laid over them ([`overlay_task_ledger`]). No records read
    /// as an empty ledger. A read never creates a file.
    pub fn read(&self) -> Result<Vec<LedgerRecordV1>> {
        let mut records = self.read_file()?;
        if let Some((db, project)) = &self.task_ledger {
            overlay_task_ledger(&mut records, db, project);
        }
        Ok(records)
    }

    /// The records as stored, without the overlay: what a write starts
    /// from.
    fn read_file(&self) -> Result<Vec<LedgerRecordV1>> {
        let rows = self.with_wrapper_rows(read_session_rows(&self.db, Some(&self.slug)));
        Ok(merge_legacy(
            self.read_legacy()?,
            rows.into_iter().map(|r| r.record).collect(),
        ))
    }

    /// The stored records, without the overlay, rendered as a JSON ledger
    /// was written ([`ExecutionStore::render_json`]): what a check of the
    /// stored bytes reads now that no JSON file is written.
    pub fn stored_json(&self) -> Result<String> {
        Self::render_json(&self.read_file()?)
    }

    /// The old JSON ledger's records. An absent or empty file is none.
    fn read_legacy(&self) -> Result<Vec<LedgerRecordV1>> {
        let raw = std::fs::read_to_string(&self.path).unwrap_or_default();
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing ledger {}", self.path.display()))
    }

    /// The bytes `ExecutionStore::write` puts on disk for `records`:
    /// `to_string_pretty`, no trailing newline, as every earlier version.
    pub fn render_json(records: &[LedgerRecordV1]) -> Result<String> {
        Ok(serde_json::to_string_pretty(records)?)
    }

    /// The ledger for people and the orchestrator LLM, newest first: what
    /// `horch sessions` prints. Every line it needs to weigh relevance and
    /// pick a resumable session id. Orchestrators come first, in their own
    /// section: they are never resumed with `horch spawn`, so they must not
    /// read as candidates among the workers. A ledger with no orchestrator
    /// record renders as before.
    pub fn render_text(records: &[LedgerRecordV1]) -> String {
        let mut records = records.to_vec();
        if records.is_empty() {
            return "no sessions recorded for this project yet\n".to_string();
        }
        records.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        records.reverse();
        let (orchestrators, workers): (Vec<LedgerRecordV1>, Vec<LedgerRecordV1>) = records
            .into_iter()
            .partition(LedgerRecordV1::is_orchestrator);
        let mut out = String::new();
        if !orchestrators.is_empty() {
            out.push_str("== orchestrators (restart with horch fleet, never resume) ==\n\n");
            render_records(&orchestrators, &mut out);
            out.push_str("== workers ==\n\n");
        }
        render_records(&workers, &mut out);
        out
    }

    fn stored_rows(&self) -> Result<Vec<SessionRow>> {
        if !self.db.is_file() {
            return Ok(self.with_wrapper_rows(Vec::new()));
        }
        let store = crate::taskledger::Store::open_read_only(&self.db)
            .with_context(|| format!("reading the session records in {}", self.db.display()))?;
        if !has_session_records_table(store.conn())? {
            return Ok(self.with_wrapper_rows(Vec::new()));
        }
        Ok(self.with_wrapper_rows(session_rows(store.conn(), Some(&self.slug))?))
    }

    fn with_wrapper_rows(&self, mut rows: Vec<SessionRow>) -> Vec<SessionRow> {
        if !channel_client::is_wrapper_process() {
            return rows;
        }
        let kept = WRAPPER_ROWS.lock().unwrap_or_else(|e| e.into_inner());
        for k in kept
            .iter()
            .filter(|k| k.db == self.db && k.slug == self.slug)
        {
            let row = SessionRow {
                project_root: self.project.clone(),
                slug: self.slug.clone(),
                json: serde_json::to_string(&k.record).unwrap_or_default(),
                record: k.record.clone(),
            };
            match rows
                .iter_mut()
                .find(|r| r.record.record_id == k.record.record_id)
            {
                Some(r) => *r = row,
                None => rows.push(row),
            }
        }
        rows
    }

    fn keep_in_wrapper(&self, rows: Vec<SessionRowChange>) {
        let mut kept = WRAPPER_ROWS.lock().unwrap_or_else(|e| e.into_inner());
        for change in rows {
            let same = |k: &WrapperRow| {
                k.db == self.db && k.slug == self.slug && k.record.record_id == change.record_id
            };
            kept.retain(|k| !same(k));
            if let Some(record) = change.record {
                kept.push(WrapperRow {
                    db: self.db.clone(),
                    slug: self.slug.clone(),
                    record,
                });
            }
        }
    }

    /// Mutate the records and send the change to fleetd, the only writer of
    /// the ledger (RR-5, T-884). The wrapper of a worker keeps the change
    /// in its process ([`Self::keep_in_wrapper`]). Else 1 `session_records` request with each
    /// changed row and the hash of each row as it was read. fleetd refuses a
    /// row that changed after the read; then `f` runs again on a new read,
    /// so 2 writers cannot lose a change. When fleetd is down, the request
    /// waits in a request file and is not committed: this prints so on
    /// stderr. A caller that needs the committed rows uses
    /// [`Self::update_written`].
    pub fn update<T>(&self, f: impl FnMut(&mut Vec<LedgerRecordV1>) -> Result<T>) -> Result<T> {
        let (out, written) = self.update_written(f)?;
        if let SessionWrite::Queued { request_id, path } = written {
            eprintln!(
                "the session records of {} are not committed: request {request_id} waits in {}. \
                 When fleetd refuses it at start, 1 needs-you item names it.",
                self.project,
                path.display()
            );
        }
        Ok(out)
    }

    /// [`Self::update`], and whether the change is committed or only
    /// queued in a request file. fleetd applies a queued change at its
    /// start, or refuses it with 1 needs-you item when a row changed after
    /// the read (RR-8, FM-15).
    pub fn update_written<T>(
        &self,
        mut f: impl FnMut(&mut Vec<LedgerRecordV1>) -> Result<T>,
    ) -> Result<(T, SessionWrite)> {
        for _ in 0..UPDATE_TRIES {
            let rows = self.stored_rows()?;
            let stored: HashMap<String, String> = rows
                .iter()
                .map(|r| (r.record.record_id.clone(), r.json.clone()))
                .collect();
            let mut records = merge_legacy(
                self.read_legacy()?,
                rows.into_iter().map(|r| r.record).collect(),
            );
            let out = f(&mut records)?;
            let changed = row_changes(&stored, &records)?;
            if changed.is_empty() {
                return Ok((out, SessionWrite::Committed));
            }
            if channel_client::is_wrapper_process() {
                self.keep_in_wrapper(changed);
                return Ok((out, SessionWrite::Committed));
            }
            if let Some(writer) = &self.writer {
                match self.apply(writer, changed) {
                    Ok(()) => return Ok((out, SessionWrite::Committed)),
                    Err(e) if e.downcast_ref::<SessionRecordChanged>().is_some() => continue,
                    Err(e) => return Err(e),
                }
            }
            match self.send(changed)? {
                Sent::Queued { request_id, path } => {
                    return Ok((out, SessionWrite::Queued { request_id, path }))
                }
                Sent::Replied(reply) => match reply.outcome {
                    Outcome::Applied(_) => return Ok((out, SessionWrite::Committed)),
                    Outcome::Refused(reason) if reason.contains(SESSION_RECORD_CHANGED) => {}
                    Outcome::Refused(reason) => bail!(
                        "fleetd refused the session records of {}: {reason}",
                        self.project
                    ),
                },
            }
        }
        bail!(
            "{SESSION_RECORD_CHANGED} {UPDATE_TRIES} times: the session records of {} are not written",
            self.project
        )
    }

    /// The payload of the change `rows`.
    fn payload(&self, rows: Vec<SessionRowChange>) -> Result<Value> {
        Ok(serde_json::to_value(SessionRecordsPayload {
            project_root: self.project.clone(),
            rows,
        })?)
    }

    /// Apply the rows `rows` in this process with the writer of fleetd.
    fn apply(&self, writer: &LedgerWriter, rows: Vec<SessionRowChange>) -> Result<()> {
        let payload = self.payload(rows)?;
        let mut store = writer.open(&self.db)?;
        store.immediate(|tx| apply_session_records(tx, &payload))?;
        Ok(())
    }

    /// Send the rows `rows` to the fleetd of [`Self::db`].
    fn send(&self, rows: Vec<SessionRowChange>) -> Result<Sent> {
        let Value::Object(payload) = self.payload(rows)? else {
            bail!("the payload of session_records is not an object");
        };
        channel_client::send(&self.fleet_context()?, Kind::SessionRecords, payload)
    }

    /// The context of the fleetd that owns [`Self::db`]: the context of the
    /// store, else of this process with the paths of the store. A file that
    /// is not the ledger of those paths is the ledger of the fleet root
    /// beside it ([`session_fleet_root`]).
    fn fleet_context(&self) -> Result<RuntimeContext> {
        let mut ctx = match &self.fleet {
            Some(ctx) => (**ctx).clone(),
            None => RuntimeContext::from_env(&crate::runtime::ProcessEnv)?,
        };
        if let Some(paths) = &self.paths {
            ctx.paths = (**paths).clone();
        }
        ctx.paths.fleet_root = session_fleet_root(&ctx.paths, &self.db);
        ctx.paths.ledger_db = self.db.clone();
        Ok(ctx)
    }

    /// Mutate every record addressed by `key` (record id or session id).
    /// Fails, naming the file, when none matches.
    pub(crate) fn update_key(
        &self,
        key: &str,
        mut f: impl FnMut(&mut LedgerRecordV1),
    ) -> Result<()> {
        self.update(|records| {
            if !records.iter().any(|r| r.matches(key)) {
                bail!("no record matches '{key}' in {}", self.db.display())
            }
            records
                .iter_mut()
                .filter(|r| r.matches(key))
                .for_each(&mut f);
            Ok(())
        })
    }

    /// Append a record as given. Lifecycle defaults are the caller's job.
    pub fn insert(&self, record: LedgerRecordV1) -> Result<()> {
        self.update(|records| {
            records.push(record.clone());
            Ok(())
        })
    }

    /// The newest record addressed by `key`.
    pub fn get(&self, key: &str) -> Result<LedgerRecordV1> {
        self.read()?
            .into_iter()
            .rfind(|r| r.matches(key))
            .with_context(|| format!("no record matches {key}"))
    }

    /// Set the typed status of the records addressed by `key`; `status`
    /// follows it. A terminal state stamps `finished_at` once.
    pub fn set_state(&self, key: &str, state: ExecutionStatus) -> Result<()> {
        let at = crate::clock::now_stamp();
        self.update_key(key, |r| {
            if state.is_terminal() {
                r.finished_at.get_or_insert_with(|| at.clone());
            } else {
                r.finished_at = None;
            }
            r.set_state(state.clone());
            r.updated_at = at.clone();
        })
    }

    /// Record the exact skills the execution was briefed with (SKL-04).
    pub fn set_skills(&self, key: &str, skills: Vec<ResolvedSkillRef>) -> Result<()> {
        self.update_key(key, |r| r.skills = skills.clone())
    }

    /// Record the launch's native-window decision (CTX-05).
    pub fn set_compact_window(&self, key: &str, window: &LedgerWindow) -> Result<()> {
        self.update_key(key, |r| r.compact_window = Some(window.clone()))
    }

    /// Every record as an [`Execution`].
    pub fn load(&self) -> Result<Vec<Execution>> {
        self.read()?
            .iter()
            .map(|r| to_execution(r).map_err(anyhow::Error::from))
            .collect()
    }

    /// Append an execution as given.
    pub fn insert_execution(&self, e: &Execution) -> Result<()> {
        self.insert(from_execution(e))
    }

    /// The pane exists and runs the worker command: `Starting`, with its pane.
    ///
    /// The worker starts as soon as `pane run` answers, before this write.
    /// On a loaded machine it can set `Running`, or even finish, first. Its
    /// state stays then: only a record that no worker has touched yet
    /// (`Planned`, or `LaunchFailed` by a spawn's recovery) becomes
    /// `Starting`. The pane is recorded either way.
    pub fn mark_starting(&self, key: &str, pane: &str) -> Result<()> {
        let at = crate::clock::now_stamp();
        self.update_key(key, |r| {
            r.pane_id = Some(pane.to_string());
            if matches!(
                r.execution_status(),
                ExecutionStatus::Planned | ExecutionStatus::LaunchFailed { .. }
            ) {
                r.finished_at = None;
                r.set_state(ExecutionStatus::Starting);
            }
            r.updated_at = at.clone();
        })
    }

    /// The worker launches its agent: `Running`. A record that has ended
    /// keeps its end: the coordinator can time out or cancel a candidate
    /// while its worker is still starting, and the closed pane then kills
    /// the worker. `LaunchFailed` (a spawn's recovery) still becomes
    /// `Running`, because the worker does run.
    pub fn mark_running(&self, key: &str) -> Result<()> {
        let at = crate::clock::now_stamp();
        self.update_key(key, |r| {
            if matches!(
                r.execution_status(),
                ExecutionStatus::Done | ExecutionStatus::Failed { .. }
            ) {
                return;
            }
            r.finished_at = None;
            r.set_state(ExecutionStatus::Running);
            r.updated_at = at.clone();
        })
    }

    /// End a record that has not ended yet with the terminal `state`. A
    /// record that has ended keeps its end: the worker can run `horch done`
    /// between a caller's read and this write. Returns the newest matching
    /// record's state after the write, which the caller records.
    pub fn end_live(&self, key: &str, state: ExecutionStatus) -> Result<ExecutionStatus> {
        debug_assert!(state.is_terminal(), "end_live needs a terminal state");
        let at = crate::clock::now_stamp();
        self.update(|records| {
            let mut last = None;
            for r in records.iter_mut().filter(|r| r.matches(key)) {
                if !r.execution_status().is_terminal() {
                    r.finished_at.get_or_insert_with(|| at.clone());
                    r.set_state(state.clone());
                    r.updated_at = at.clone();
                }
                last = Some(r.execution_status());
            }
            last.with_context(|| format!("no record matches '{key}' in {}", self.db.display()))
        })
    }

    /// The agent process ended with `code` (`None`: killed by a signal, or it
    /// never started). A live record ends: `Done` on 0, and
    /// `Failed(AgentExited)` otherwise, unless it is an orchestrator, which
    /// keeps its state. A record that is already terminal (the worker ran
    /// `horch done`) only gets the code.
    ///
    /// True when it ended the record: the agent left without `horch done`,
    /// so nobody told the orchestrator.
    pub fn record_exit(&self, key: &str, code: Option<i32>) -> Result<bool> {
        let at = crate::clock::now_stamp();
        let mut ended = false;
        self.update_key(key, |r| ended = exit_record(r, code, &at))?;
        Ok(ended)
    }

    /// The recovery rule for a spawner that stopped between the insert and
    /// the pane (a crash, or `HORCH_FAULT=abort-after-*`): a `Planned`
    /// record without a pane whose last update is [`ABANDONED_AFTER`] or more
    /// before `now` becomes `LaunchFailed`. Its role and brief are freed with
    /// it. Returns the records it closed. Every `horch spawn` runs it first.
    ///
    /// The stage is the last one that may have run; the record cannot say
    /// which did. A `Planned` record is never live, so until this runs only
    /// the legacy `status` (`working`) is stale.
    pub fn recover_abandoned(&self, now: DateTime<Utc>) -> Result<Vec<LedgerRecordV1>> {
        let abandoned = |r: &LedgerRecordV1| {
            r.state == Some(ExecutionStatus::Planned)
                && r.pane_id.is_none()
                && crate::clock::parse(&r.updated_at).is_some_and(|t| now - t >= ABANDONED_AFTER)
        };
        // Read first: most spawns find nothing, and then write nothing.
        if !self.read_file()?.iter().any(abandoned) {
            return Ok(Vec::new());
        }
        let at = crate::clock::stamp(now);
        self.update(|records| {
            let mut closed = Vec::new();
            for r in records.iter_mut().filter(|r| abandoned(r)) {
                r.set_state(ExecutionStatus::LaunchFailed {
                    stage: LaunchStage::Split,
                    reason: "abandoned: the spawner stopped before the worker's pane ran"
                        .to_string(),
                });
                r.finished_at = Some(at.clone());
                r.updated_at = at.clone();
                closed.push(r.clone());
            }
            Ok(closed)
        })
    }

    /// End the newest record addressed by `key` when its pane closed without
    /// `horch done`: the record is still `working`, names a pane, and herdr
    /// answers but does not have that pane. The record becomes
    /// `Failed(PaneVanished)` with the history text [`PANE_CLOSED`], so
    /// `horch spawn --resume` can reopen it. A pane that is open, a record
    /// without a pane, an orchestrator, and a herdr that does not answer
    /// leave the record alone. Returns whether it ended the record.
    pub fn end_if_pane_closed(&self, key: &str, ws: &dyn WorkspaceClient) -> Result<bool> {
        let at = crate::clock::now_stamp();
        match self.closed_working_record(key, ws) {
            None => Ok(false),
            Some(record) => self.update(|rs| Ok(mark_pane_vanished(rs, &record.record_id, &at))),
        }
    }

    /// The newest record `key` addresses, when it is a working worker whose
    /// pane herdr (answering) does not have.
    fn closed_working_record(&self, key: &str, ws: &dyn WorkspaceClient) -> Option<LedgerRecordV1> {
        let record = self.get(key).ok()?;
        if record.status != STATUS_WORKING || record.is_orchestrator() {
            return None;
        }
        let pane = record.pane_id.as_deref()?;
        let gone = ws.pane_get(pane).is_err() && ws.server_reachable();
        gone.then_some(record)
    }

    /// Records whose typed status is live: starting or running.
    pub fn live(&self) -> Result<Vec<LedgerRecordV1>> {
        Ok(self
            .read()?
            .into_iter()
            .filter(|r| r.execution_status().is_live())
            .collect())
    }
}

/// End the still-working rows of `record_id` as `PaneVanished`. Re-checked
/// under the lock: the worker may run `horch done` now.
fn mark_pane_vanished(records: &mut [LedgerRecordV1], record_id: &str, at: &str) -> bool {
    let mut ended = false;
    for r in records.iter_mut() {
        if r.record_id != record_id || r.status != STATUS_WORKING {
            continue;
        }
        r.set_state(ExecutionStatus::Failed {
            failure: FailureKind::PaneVanished,
        });
        if r.finished_at.is_none() {
            r.finished_at = Some(at.to_string());
        }
        r.updated_at = at.to_string();
        r.history.push(HistoryEntry {
            at: at.to_string(),
            event: "ended".to_string(),
            text: PANE_CLOSED.to_string(),
        });
        ended = true;
    }
    ended
}

/// The text of each record, in the given order.
fn render_records(records: &[LedgerRecordV1], out: &mut String) {
    for r in records {
        let session = r.session_id.as_deref().unwrap_or("not-yet-known");
        out.push_str(&format!(
            "[{}] {} ({})  session={}  record={}\n",
            r.status, r.tier, r.role, session, r.record_id
        ));
        let effort = r
            .effort
            .as_deref()
            .map(|e| format!(" effort={e}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "  agent={} model={}{effort}  updated={}\n",
            r.agent, r.model, r.updated_at
        ));
        if let Some(phase) = r.phase {
            out.push_str(&format!("  phase={phase}\n"));
        }
        if let Some(via) = &r.via {
            out.push_str(&format!(
                "  ran via {via}: {}\n",
                r.substitution_reason.as_deref().unwrap_or("substituted")
            ));
        }
        out.push_str(&format!("  task: {}\n", r.task.replace('\n', "\n        ")));
        let notable: Vec<&crate::execution::legacy::HistoryEntry> = r
            .history
            .iter()
            .filter(|h| {
                h.event == "done"
                    || h.event == "note"
                    || h.event == "verdict"
                    || h.event == crate::execution::lifecycle::EVENT_STARTUP_FAILED
            })
            .collect();
        for h in notable.iter().rev().take(3).rev() {
            out.push_str(&format!(
                "  {} @ {}: {}\n",
                h.event,
                h.at,
                h.text.replace(
                    '\n',
                    &format!("\n  {}", " ".repeat(h.event.len() + h.at.len() + 5))
                )
            ));
        }
        out.push('\n');
    }
}

/// 1 worker row of the task ledger, with the task it claimed.
struct LedgerWorker {
    worker: crate::taskledger::workers::Worker,
    /// `T-<seq>: <title>` of the claimed task.
    task: Option<String>,
}

/// The worker rows of every run of `project` in the task ledger `db`, newest
/// first. A ledger file that does not exist, or that does not open or read
/// right now, gives none: a session read never fails on the task ledger.
fn ledger_workers(db: &Path, project: &Path) -> Vec<LedgerWorker> {
    if !db.is_file() {
        return Vec::new();
    }
    let read = || -> Result<Vec<LedgerWorker>> {
        let store = crate::taskledger::Store::open_read_only(db)?;
        let mut stmt = store.conn().prepare(
            "SELECT w.*, t.seq AS task_seq, t.title AS task_title
             FROM workers w
             JOIN runs r ON r.run_id = w.run_id
             JOIN projects p ON p.project_id = r.project_id
             LEFT JOIN tasks t ON t.task_id = w.task_id
             WHERE p.root = ?1
             ORDER BY w.spawned_at DESC, w.rowid DESC",
        )?;
        let root = crate::taskledger::runs::root_key(project);
        let rows = stmt
            .query_map([root], |row| {
                let seq: Option<i64> = row.get("task_seq")?;
                let title: Option<String> = row.get("task_title")?;
                Ok(LedgerWorker {
                    worker: crate::taskledger::workers::Worker::from_row(row)?,
                    task: seq.zip(title).map(|(s, t)| format!("T-{s}: {t}")),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    };
    read().unwrap_or_default()
}

/// The typed status of a worker row.
fn worker_state(w: &crate::taskledger::workers::Worker) -> ExecutionStatus {
    use crate::taskledger::model::WorkerStatus as W;
    let reason = || w.end_reason.clone().unwrap_or_else(|| w.status.clone());
    match W::parse(&w.status) {
        Ok(W::Spawning | W::Starting) => ExecutionStatus::Starting,
        Ok(W::Claiming | W::Working) => ExecutionStatus::Running,
        Ok(W::Done | W::NoTask) => ExecutionStatus::Done,
        Ok(W::LaunchFailed) => ExecutionStatus::LaunchFailed {
            stage: LaunchStage::Run,
            reason: reason(),
        },
        Ok(W::Stopped) => ExecutionStatus::Failed {
            failure: FailureKind::Cancelled { reason: reason() },
        },
        Ok(W::Incomplete) | Err(_) => ExecutionStatus::Failed {
            failure: FailureKind::AgentExited {
                code: w.exit_code.and_then(|c| i32::try_from(c).ok()),
            },
        },
    }
}

/// The newest of a worker row's stamps.
fn worker_updated(w: &crate::taskledger::workers::Worker) -> Option<String> {
    [
        &w.spawned_at,
        &w.started_at,
        &w.claimed_at,
        &w.last_progress_at,
        &w.ended_at,
    ]
    .into_iter()
    .flatten()
    .max()
    .cloned()
}

/// The JSON record a worker row stands for: the record it names, else the
/// newest unclaimed record of its kind, role, workspace and liveness. An
/// unnamed row matches only a record that no other row names.
fn matching_record(
    records: &[LedgerRecordV1],
    w: &crate::taskledger::workers::Worker,
    taken: &std::collections::HashSet<usize>,
    named: &std::collections::HashSet<String>,
) -> Option<usize> {
    if let Some(id) = &w.legacy_record_id {
        return records.iter().rposition(|r| &r.record_id == id);
    }
    let kind = if w.kind == KIND_ORCHESTRATOR {
        KIND_ORCHESTRATOR
    } else {
        KIND_WORKER
    };
    let live = worker_state(w).is_live();
    records
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            !taken.contains(i)
                && !named.contains(&r.record_id)
                && r.kind == kind
                && Some(r.role.as_str()) == w.role.as_deref()
                && r.workspace_id.is_some()
                && r.workspace_id == w.workspace_id
                && r.execution_status().is_live() == live
        })
        .max_by(|(_, a), (_, b)| a.updated_at.cmp(&b.updated_at))
        .map(|(i, _)| i)
}

/// Lay a worker row over its JSON record. The row wins for what it records:
/// the claimed task, liveness, session, pane and end. The record keeps what
/// only it has: history, skills, routing and the window decision.
fn overlay_record(r: &mut LedgerRecordV1, lw: &LedgerWorker) {
    let w = &lw.worker;
    if let Some(task) = &lw.task {
        r.task = task.clone();
    }
    let state = worker_state(w);
    if state.is_live() != r.execution_status().is_live() {
        r.set_state(state.clone());
    }
    if state.is_terminal() {
        if r.finished_at.is_none() {
            r.finished_at = w.ended_at.clone();
        }
    } else {
        r.finished_at = None;
    }
    let fill = |field: &mut Option<String>, value: &Option<String>| {
        if value.is_some() {
            *field = value.clone();
        }
    };
    fill(&mut r.session_id, &w.session_id);
    fill(&mut r.pane_id, &w.pane_id);
    fill(&mut r.workspace_id, &w.workspace_id);
    if r.effort.is_none() {
        r.effort = w.effort.clone();
    }
    if r.agent.is_empty() {
        r.agent = w.harness.clone().unwrap_or_default();
    }
    if r.model.is_empty() {
        r.model = w.model.clone().unwrap_or_default();
    }
    if r.exit_code.is_none() {
        r.exit_code = w.exit_code.and_then(|c| i32::try_from(c).ok());
    }
    if let (Some(summary), Some(at)) = (&w.summary, &w.ended_at) {
        if !r
            .history
            .iter()
            .any(|h| h.event == "done" && &h.text == summary)
        {
            r.history.push(HistoryEntry {
                at: at.clone(),
                event: "done".to_string(),
                text: summary.clone(),
            });
        }
    }
    if let Some(at) = worker_updated(w).filter(|at| *at > r.updated_at) {
        r.updated_at = at;
    }
}

/// The record of a worker row that has no JSON record: its id is the
/// worker id. An orchestrator row without a harness takes the harness of
/// the newest JSON orchestrator record of its workspace.
fn synthesized_record(lw: &LedgerWorker, records: &[LedgerRecordV1]) -> LedgerRecordV1 {
    let w = &lw.worker;
    let orchestrator = w.kind == KIND_ORCHESTRATOR;
    let agent = w.harness.clone().or_else(|| {
        records
            .iter()
            .filter(|r| r.is_orchestrator() && orchestrator && r.workspace_id == w.workspace_id)
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
            .map(|r| r.agent.clone())
    });
    let spawned = w.spawned_at.clone().unwrap_or_default();
    let mut r = LedgerRecordV1 {
        record_id: w.worker_id.clone(),
        session_id: w.session_id.clone(),
        agent: agent.unwrap_or_default(),
        tier: w.persona.clone().unwrap_or_default(),
        model: w.model.clone().unwrap_or_default(),
        effort: w.effort.clone(),
        role: w.label().to_string(),
        task: lw.task.clone().unwrap_or_else(|| {
            if orchestrator {
                crate::execution::records::ORCHESTRATING_TASK.to_string()
            } else {
                crate::execution::records::IDLE_TASK.to_string()
            }
        }),
        history: vec![HistoryEntry {
            at: spawned.clone(),
            event: "spawned".to_string(),
            text: format!("task-ledger worker {}", w.worker_id),
        }],
        created_at: spawned.clone(),
        updated_at: spawned,
        kind: if orchestrator {
            KIND_ORCHESTRATOR
        } else {
            KIND_WORKER
        }
        .to_string(),
        workspace_id: w.workspace_id.clone(),
        pane_id: w.pane_id.clone(),
        ..LedgerRecordV1::default()
    };
    r.set_state(worker_state(w));
    overlay_record(&mut r, lw);
    r
}

/// Lay the task ledger's worker rows for `project` over `records` (TRF-44,
/// D10 step 1). A row that names a record, or matches one by kind, role and
/// workspace, updates it in place (`overlay_record`). A row without a
/// record is added after them (`synthesized_record`). Without the ledger
/// file the records stay as read.
pub fn overlay_task_ledger(records: &mut Vec<LedgerRecordV1>, db: &Path, project: &Path) {
    let rows = ledger_workers(db, project);
    if rows.is_empty() {
        return;
    }
    let named: std::collections::HashSet<String> = rows
        .iter()
        .filter_map(|lw| lw.worker.legacy_record_id.clone())
        .collect();
    let mut taken = std::collections::HashSet::new();
    let mut added = Vec::new();
    for lw in &rows {
        match matching_record(records, &lw.worker, &taken, &named) {
            Some(i) if taken.insert(i) => overlay_record(&mut records[i], lw),
            // A second row of 1 record (a resume) leaves it to the newest.
            Some(_) => {}
            None => added.push(synthesized_record(lw, records)),
        }
    }
    added.reverse();
    records.extend(added);
}

#[cfg(test)]
mod trf_44_tests {
    use super::*;
    use crate::execution::records::{NewRecord, SessionLedger};
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::workers::{self, NewWorker};
    use crate::taskledger::Store;

    struct Fixture {
        /// Declared first, so it drops before the folder.
        _fleetd: crate::fleetd::fleet_runtime::adapters::channel_server::ChannelServer,
        _tmp: tempfile::TempDir,
        project: PathBuf,
        db: PathBuf,
        ledger: SessionLedger,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("repo");
        std::fs::create_dir_all(&project).unwrap();
        let db = tmp.path().join("ledger.sqlite3");
        let state = tmp.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let fleetd = test_serve(&state.join(STATE_DB));
        let ledger = SessionLedger::for_project(&state, project.to_str().unwrap())
            .with_task_ledger(&db, &project);
        Fixture {
            _fleetd: fleetd,
            _tmp: tmp,
            project,
            db,
            ledger,
        }
    }

    fn add_record(f: &Fixture, id: &str, role: &str, task: &str) {
        f.ledger
            .add(NewRecord {
                record_id: id,
                agent: "claude",
                tier: "opus",
                model: "opus",
                role,
                session_id: None,
                task,
                phase: None,
            })
            .unwrap();
        f.ledger
            .store()
            .update_key(id, |r| r.workspace_id = Some("w1".into()))
            .unwrap();
    }

    /// A run with task T-1 and 1 worker row; `set` is the SQL `SET` list
    /// for the row, with `?2` the task id.
    fn add_worker(f: &Fixture, new: NewWorker, set: &str) -> String {
        let mut store = Store::open(&f.db).unwrap();
        store
            .immediate(|tx| {
                let run = match runs::of_project(tx, &f.project)?.into_iter().next() {
                    Some(run) => run,
                    None => runs::create(
                        tx,
                        &f.project,
                        &NewRun {
                            slug: "r1".into(),
                            ..Default::default()
                        },
                    )?,
                };
                // FR-12: a task needs an idea in flight of the feature of its run.
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                let task = match tasks::resolve(tx, &run.run_id, "T-1") {
                    Ok(t) => t,
                    Err(_) => tasks::add(
                        tx,
                        &run,
                        &NewTask::work("Fix the parser", "t.md", "opus"),
                        Actor::Orchestrator,
                    )?,
                };
                let id = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        workspace_id: Some("w1".into()),
                        ..new
                    },
                )?;
                let sql = format!("UPDATE workers SET {set} WHERE worker_id = ?1");
                if sql.contains("?2") {
                    tx.execute(&sql, rusqlite::params![id, task.task_id])?;
                } else {
                    tx.execute(&sql, [&id])?;
                }
                Ok(id)
            })
            .unwrap()
    }

    fn worker(role: &str, record: Option<&str>) -> NewWorker {
        NewWorker {
            persona: "opus".into(),
            role: Some(role.into()),
            legacy_record_id: record.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn trf_44_without_the_ledger_file_records_read_as_written() {
        let f = fixture();
        add_record(&f, "r1", "opus-1", "Run horch ledger claim.");
        assert!(!f.db.exists());
        let read = f.ledger.read().unwrap();
        let raw = f.ledger.store().read_file().unwrap();
        assert_eq!(
            ExecutionStore::render_json(&read).unwrap(),
            ExecutionStore::render_json(&raw).unwrap()
        );
        assert!(!f.db.exists(), "a read must not create the task ledger");
    }

    #[test]
    fn trf_44_a_linked_worker_shows_its_claimed_task() {
        let f = fixture();
        add_record(&f, "r1", "opus-1", "Run horch ledger claim.");
        add_worker(
            &f,
            worker("opus-1", Some("r1")),
            "status = 'working', task_id = ?2, session_id = 's-1'",
        );
        let records = f.ledger.read().unwrap();
        assert_eq!(records.len(), 1, "{records:?}");
        assert_eq!(records[0].task, "T-1: Fix the parser");
        assert_eq!(records[0].session_id.as_deref(), Some("s-1"));
        assert!(records[0].execution_status().is_live());
        let text = ExecutionStore::render_text(&records);
        assert!(text.contains("task: T-1: Fix the parser"), "{text}");
        // The JSON file keeps what the writers wrote.
        assert_eq!(
            f.ledger.store().read_file().unwrap()[0].task,
            "Run horch ledger claim."
        );
    }

    #[test]
    fn trf_44_an_ended_worker_row_ends_its_record_with_its_summary() {
        let f = fixture();
        add_record(&f, "r1", "opus-1", "Run horch ledger claim.");
        add_worker(
            &f,
            worker("opus-1", Some("r1")),
            "status = 'done', task_id = ?2, summary = 'fixed it', ended_at = '2099-01-01T00:00:00Z'",
        );
        let r = f.ledger.get("r1").unwrap();
        assert_eq!(r.status, "done");
        assert_eq!(r.finished_at.as_deref(), Some("2099-01-01T00:00:00Z"));
        assert!(r
            .history
            .iter()
            .any(|h| h.event == "done" && h.text == "fixed it"));
    }

    #[test]
    fn trf_44_a_worker_row_without_a_record_is_a_session() {
        let f = fixture();
        let id = add_worker(
            &f,
            NewWorker {
                harness: Some("codex".into()),
                model: Some("gpt".into()),
                ..worker("codex-1", None)
            },
            "status = 'working', task_id = ?2",
        );
        let records = f.ledger.read().unwrap();
        assert_eq!(records.len(), 1, "{records:?}");
        let r = &records[0];
        assert_eq!(r.record_id, id);
        assert_eq!((r.tier.as_str(), r.role.as_str()), ("opus", "codex-1"));
        assert_eq!((r.agent.as_str(), r.model.as_str()), ("codex", "gpt"));
        assert_eq!(r.task, "T-1: Fix the parser");
        assert!(r.execution_status().is_live());
        assert_eq!(
            f.ledger
                .live_for_role("codex-1", Some("w1"))
                .unwrap()
                .unwrap()
                .record_id,
            id
        );
    }

    #[test]
    fn trf_44_an_orchestrator_row_is_its_record_and_shows_its_harness() {
        let f = fixture();
        f.ledger
            .insert(LedgerRecordV1 {
                record_id: "o1".into(),
                agent: "claude".into(),
                tier: "orchestrator".into(),
                model: "opus".into(),
                role: "orchestrator".into(),
                task: crate::execution::records::ORCHESTRATING_TASK.into(),
                kind: KIND_ORCHESTRATOR.into(),
                workspace_id: Some("w1".into()),
                ..LedgerRecordV1::default()
            })
            .unwrap();
        add_worker(
            &f,
            NewWorker {
                kind: Some(crate::taskledger::model::WorkerKind::Orchestrator),
                persona: "orchestrator".into(),
                ..worker("orchestrator", None)
            },
            "status = 'working'",
        );
        let records = f.ledger.read().unwrap();
        assert_eq!(records.len(), 1, "{records:?}");
        assert_eq!(records[0].record_id, "o1");
        assert_eq!(records[0].agent, "claude");
    }

    #[test]
    fn trf_44_an_orchestrator_row_without_a_record_takes_a_harness() {
        let f = fixture();
        f.ledger
            .insert(LedgerRecordV1 {
                record_id: "o-old".into(),
                agent: "codex".into(),
                role: "orchestrator".into(),
                kind: KIND_ORCHESTRATOR.into(),
                workspace_id: Some("w1".into()),
                ..LedgerRecordV1::default()
            })
            .unwrap();
        f.ledger.done("o-old", "superseded").unwrap();
        let orchestrator = || NewWorker {
            kind: Some(crate::taskledger::model::WorkerKind::Orchestrator),
            persona: "orchestrator".into(),
            ..worker("orchestrator", None)
        };
        // The stopped row is the done record; the working row has none.
        add_worker(
            &f,
            orchestrator(),
            "status = 'stopped', spawned_at = '2000-01-01T00:00:00Z'",
        );
        let newer = add_worker(
            &f,
            orchestrator(),
            "status = 'working', spawned_at = '2099-01-01T00:00:00Z'",
        );
        let records = f.ledger.read().unwrap();
        assert_eq!(records.len(), 2, "{records:?}");
        let synth = records.iter().find(|r| r.record_id == newer).unwrap();
        assert!(synth.is_orchestrator());
        assert_eq!(synth.agent, "codex");
    }
}

/// Serve `session_records` for the ledger `db` on the fleet root beside it,
/// where [`ExecutionStore::update`] of a store with no context finds it.
#[cfg(test)]
pub(crate) fn test_serve(
    db: &Path,
) -> crate::fleetd::fleet_runtime::adapters::channel_server::ChannelServer {
    crate::fleetd::fleet_runtime::adapters::test_fleetd::serve_session_records_at(
        db,
        &db.parent().unwrap().join("fleet"),
    )
    .unwrap()
}

#[cfg(test)]
mod trf_45_tests {
    use super::*;
    use crate::execution::records::{NewRecord, SessionLedger};

    fn json_files(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.ends_with(".json"))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn add(ledger: &SessionLedger, id: &str) {
        ledger
            .add(NewRecord {
                record_id: id,
                agent: "claude",
                tier: "opus",
                model: "opus",
                role: "opus-1",
                session_id: None,
                task: "t",
                phase: None,
            })
            .unwrap();
    }

    /// Spawn, worker start and exit, note and done write the session
    /// records and never a JSON file.
    #[test]
    fn trf_45_every_session_write_goes_to_the_table_and_no_json_file() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let _fleetd = test_serve(&state.join(STATE_DB));
        let ledger = SessionLedger::for_project(&state, "/repo");
        let store = ledger.store().clone();
        add(&ledger, "r-1");
        store.set_skills("r-1", Vec::new()).unwrap();
        store.mark_starting("r-1", "w1:p2").unwrap();
        store.mark_running("r-1").unwrap();
        ledger.note("r-1", "progress").unwrap();
        ledger.done("r-1", "finished").unwrap();
        store.record_exit("r-1", Some(0)).unwrap();
        add(&ledger, "r-2");
        store
            .end_live(
                "r-2",
                ExecutionStatus::Failed {
                    failure: FailureKind::PaneVanished,
                },
            )
            .unwrap();
        store.recover_abandoned(crate::clock::now()).unwrap();

        assert_eq!(json_files(&state), Vec::<String>::new());
        let records = store.read().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[0].history.iter().any(|h| h.text == "finished"));
        assert_eq!(records[0].pane_id.as_deref(), Some("w1:p2"));
        let conn = crate::taskledger::Store::open(store.db()).unwrap();
        let rows: i64 = conn
            .conn()
            .query_row(
                "SELECT count(*) FROM session_records WHERE slug = '-repo' AND project_root = '/repo'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 2);
    }

    /// An old JSON ledger reads as before, is never written, and its
    /// records move into the table at the next write.
    #[test]
    fn trf_45_an_old_json_ledger_is_read_and_never_written() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ExecutionStore::for_project(tmp.path(), "/repo");
        let old = LedgerRecordV1 {
            record_id: "old-1".into(),
            role: "opus-1".into(),
            status: STATUS_WORKING.into(),
            ..LedgerRecordV1::default()
        };
        let text = ExecutionStore::render_json(&[old]).unwrap();
        std::fs::write(store.path(), &text).unwrap();
        assert!(!store.db().exists());
        assert_eq!(store.read().unwrap()[0].record_id, "old-1");
        assert!(!store.db().exists(), "a read creates no file");

        let _fleetd = test_serve(store.db());
        store.mark_running("old-1").unwrap();
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), text);
        let rows = read_session_rows(store.db(), Some("-repo"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].record.execution_status(), ExecutionStatus::Running);
        assert_eq!(store.read().unwrap().len(), 1, "no duplicate");
    }

    /// A worker row that names a record links the session record to it.
    #[test]
    fn trf_45_a_session_record_names_its_worker_row() {
        use crate::taskledger::runs::{self, NewRun};
        use crate::taskledger::workers::{self, NewWorker};
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("ledger.sqlite3");
        let mut ledger = crate::taskledger::Store::open(&db).unwrap();
        let wid = ledger
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                workers::insert(
                    tx,
                    &workers::NewWorker {
                        run_id: run.run_id,
                        persona: "opus".into(),
                        legacy_record_id: Some("r-1".into()),
                        ..NewWorker::default()
                    },
                )
            })
            .unwrap();
        let _fleetd = test_serve(&db);
        let store = ExecutionStore::in_db(tmp.path(), &db, "/repo");
        store
            .insert(LedgerRecordV1 {
                record_id: "r-1".into(),
                ..LedgerRecordV1::default()
            })
            .unwrap();
        let linked: Option<String> = ledger
            .conn()
            .query_row(
                "SELECT worker_id FROM session_records WHERE record_id = 'r-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, Some(wid));
    }

    /// A state dir moved alone keeps its sessions there; the default state
    /// dir uses the task ledger. An explicit `HORCH_LEDGER_DB` wins over a
    /// moved state dir (T-890). This changes the old rule "a moved state dir
    /// wins over HORCH_LEDGER_DB": after T-884 only fleetd writes the session
    /// rows, so they must be in the ledger of that fleetd.
    #[test]
    fn trf_45_session_db_is_the_task_ledger_unless_the_state_dir_moved() {
        use crate::runtime::MapEnv;
        let home = Path::new("/home/u");
        let at = |env: MapEnv| Paths::from_env(&env.with("HOME", "/home/u"));
        let default = at(MapEnv::new("/"));
        assert_eq!(session_db(&default), home.join(".horch/ledger.sqlite3"));
        let named = at(MapEnv::new("/").with("HORCH_STATE_DIR", "/home/u/.local/state/horch"));
        assert_eq!(session_db(&named), home.join(".horch/ledger.sqlite3"));
        let scratch = at(MapEnv::new("/").with("HORCH_LEDGER_DB", "/tmp/l.sqlite3"));
        assert_eq!(session_db(&scratch), Path::new("/tmp/l.sqlite3"));
        let moved = at(MapEnv::new("/").with("HORCH_STATE_DIR", "/tmp/s"));
        assert_eq!(session_db(&moved), Path::new("/tmp/s/ledger.sqlite3"));
        let explicit = at(MapEnv::new("/")
            .with("HORCH_STATE_DIR", "/tmp/s")
            .with("HORCH_LEDGER_DB", "/tmp/l.sqlite3"));
        assert_eq!(session_db(&explicit), Path::new("/tmp/l.sqlite3"));
    }
}

/// The request kind `session_records` (T-884): its payload check, the
/// hash check of its handler, and the retry of [`ExecutionStore::update`].
#[cfg(test)]
mod session_records_tests {
    use std::cell::Cell;

    use serde_json::json;

    use super::*;

    fn record(id: &str) -> LedgerRecordV1 {
        LedgerRecordV1 {
            record_id: id.into(),
            role: "opus-1".into(),
            status: STATUS_WORKING.into(),
            ..LedgerRecordV1::default()
        }
    }

    /// Apply `payload` in 1 transaction of the ledger `db`.
    fn apply(db: &Path, payload: Value) -> Result<Value> {
        let mut store = crate::taskledger::Store::open(db).unwrap();
        store.immediate(|tx| apply_session_records(tx, &payload))
    }

    #[test]
    fn session_records_refuses_a_payload_of_a_bad_form() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("ledger.sqlite3");
        let r1 = serde_json::to_value(record("r1")).unwrap();
        let cases = [
            (json!({"project_root": "/repo", "rows": []}), "has no row"),
            (
                json!({"project_root": " ", "rows": [{"record_id": "r1", "record": r1}]}),
                "project_root",
            ),
            (
                json!({"project_root": "/repo", "rows": [{"record_id": "../x", "record": null}]}),
                "is not 1 to 64",
            ),
            (
                json!({"project_root": "/repo", "rows": [
                    {"record_id": "r1", "record": r1},
                    {"record_id": "r1", "record": null}]}),
                "2 times",
            ),
            (
                json!({"project_root": "/repo", "rows": [{"record_id": "r2", "record": r1}]}),
                "another record_id",
            ),
            (
                json!({"project_root": "/repo", "rows": [{"record_id": "r1", "expected": "x"}]}),
                "not a sha256",
            ),
            (
                json!({"project_root": "/repo", "rows": [], "slug": "-repo"}),
                "does not fit its kind",
            ),
        ];
        for (payload, reason) in cases {
            let e = apply(&db, payload.clone()).unwrap_err();
            assert!(format!("{e:#}").contains(reason), "{payload}: {e:#}");
        }
        let rows = read_session_rows(&db, None);
        assert!(rows.is_empty(), "a refused payload writes nothing");
    }

    #[test]
    fn session_records_applies_a_row_only_when_it_is_as_read() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("ledger.sqlite3");
        let r1 = serde_json::to_value(record("r1")).unwrap();
        let put = |expected: Value| {
            json!({"project_root": "/repo", "rows": [
            {"record_id": "r1", "expected": expected, "record": r1}]})
        };
        apply(&db, put(Value::Null)).unwrap();
        let e = apply(&db, put(Value::Null)).unwrap_err();
        assert!(format!("{e:#}").contains(SESSION_RECORD_CHANGED), "{e:#}");
        let stored = read_session_rows(&db, Some("-repo")).remove(0).json;
        apply(&db, put(json!(row_hash(&stored)))).unwrap();
        let other = json!({"project_root": "/other", "rows": [
            {"record_id": "r1", "expected": row_hash(&stored), "record": null}]});
        let e = apply(&db, other).unwrap_err();
        assert!(
            format!("{e:#}").contains("is a record of the project slug -repo"),
            "{e:#}"
        );
        let remove = json!({"project_root": "/repo", "rows": [
            {"record_id": "r1", "expected": row_hash(&stored), "record": null}]});
        apply(&db, remove).unwrap();
        assert!(read_session_rows(&db, None).is_empty());
    }

    /// A row that another writer changed after the read is refused, and
    /// the update reads again and runs its change on the new row: no
    /// change is lost.
    #[test]
    fn update_reads_again_after_a_refused_change() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join(STATE_DB);
        let _fleetd = test_serve(&db);
        let store = ExecutionStore::for_project(tmp.path(), "/repo");
        let other = store.clone();
        store.insert(record("r1")).unwrap();
        let runs = Cell::new(0);
        store
            .update_key("r1", |r| {
                runs.set(runs.get() + 1);
                if runs.get() == 1 {
                    other
                        .update_key("r1", |r| r.effort = Some("other".into()))
                        .unwrap();
                }
                r.pane_id = Some("w1:p2".into());
            })
            .unwrap();
        assert_eq!(runs.get(), 2, "the refused change runs again");
        let r = store.get("r1").unwrap();
        assert_eq!(r.effort.as_deref(), Some("other"));
        assert_eq!(r.pane_id.as_deref(), Some("w1:p2"));
    }
}
