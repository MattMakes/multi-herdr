//! The execution store: a project's ledger file, read and written whole.
//!
//! One JSON array of [`LedgerRecordV1`] at `<state_root>/<slug>.json`, the
//! path, slug rule and pretty-printed format the bash implementation and
//! every earlier binary used. A read-modify-write holds `DirLock` on
//! `<slug>.json.lock/`, the directory the old mkdir spinlock created, so an
//! old binary and a new one still exclude each other. Writes go through
//! [`fsx::write_atomic`]: temp file, fsync, rename, directory fsync.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use chrono::{DateTime, Utc};

use crate::execution::legacy::{LedgerRecordV1, KIND_ORCHESTRATOR, KIND_WORKER};
use crate::execution::model::{
    Execution, ExecutionKind, ExecutionStatus, FailureKind, LaunchStage, SessionState, Task,
};
use crate::fsx::{self, DirLock};
use crate::ids::IdError;
use crate::runtime::Paths;
use crate::skills::activation::ResolvedSkillRef;

/// A lock older than this is broken: panes can be killed mid-write, and the
/// old spinlock broke after about 15 seconds too.
const LOCK_STALE_AFTER: Duration = Duration::from_secs(15);
/// How long a writer waits for a live lock before it gives up.
const LOCK_TIMEOUT: Duration = Duration::from_secs(60);
/// The ledger file's mode: what `std::fs::write` gave it under the usual
/// umask before the store wrote it.
const LEDGER_MODE: u32 = 0o644;

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
/// `SPEC-TODO(Spec B)`: whether the judge attempt needs its own key.
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

fn kind_of(r: &LedgerRecordV1) -> Result<ExecutionKind, LegacyError> {
    let bad = |reason: &str| LegacyError::Kind {
        record: r.record_id.clone(),
        reason: reason.to_string(),
    };
    if r.kind == KIND_ORCHESTRATOR {
        return match (&r.experiment_id, &r.round_id, &r.label) {
            (None, None, None) => Ok(ExecutionKind::Orchestrator),
            _ => Err(bad("an orchestrator has no experiment, round or label")),
        };
    }
    if r.kind != KIND_WORKER {
        return Err(bad(&format!("unknown kind '{}'", r.kind)));
    }
    match (&r.experiment_id, &r.round_id, &r.label) {
        (None, None, None) => Ok(ExecutionKind::Worker),
        (None, Some(round), Some(label)) if label.starts_with(JUDGE_LABEL) => {
            let attempt = label[JUDGE_LABEL.len()..]
                .parse()
                .map_err(|_| bad(&format!("judge label '{label}' has no attempt number")))?;
            Ok(ExecutionKind::Judge {
                round: round.parse()?,
                attempt,
            })
        }
        (Some(experiment), Some(round), Some(label)) => Ok(ExecutionKind::Candidate {
            experiment: experiment.parse()?,
            round: round.parse()?,
            label: label.clone(),
        }),
        _ => Err(bad("experiment_id, round_id and label do not name a kind")),
    }
}

/// The typed view of a ledger record. Every key of the record has a place in
/// [`Execution`], so [`from_execution`] gives the record back unchanged.
///
/// A `session_id` of `""` (the bash spelling for none) reads as
/// [`SessionState::Unavailable`] and writes back as `""`.
pub fn to_execution(r: &LedgerRecordV1) -> Result<Execution, LegacyError> {
    let path = |p: &Option<String>| p.as_ref().map(std::path::PathBuf::from);
    Ok(Execution {
        id: r.record_id.parse()?,
        kind: kind_of(r)?,
        teammate: r.tier.parse()?,
        harness: r.agent.parse().map_err(LegacyError::Agent)?,
        model: r.model.clone(),
        effort: r.effort.clone(),
        phase: r.phase,
        role: r.role.parse()?,
        task: Task {
            id: r.task_id.as_deref().map(str::parse).transpose()?,
            text: r.task.clone(),
            plan: r.plan.clone(),
        },
        status: r.execution_status(),
        typed_status: r.state.is_some(),
        session: match r.session_id.as_deref() {
            None => SessionState::Pending,
            Some("") => SessionState::Unavailable,
            Some(id) => SessionState::Known(id.parse()?),
        },
        exit_code: r.exit_code,
        project: path(&r.project),
        workdir: path(&r.workdir),
        workspace: r.workspace_id.as_deref().map(str::parse).transpose()?,
        pane: r.pane_id.as_deref().map(str::parse).transpose()?,
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
    }
}

/// A project's executions on disk.
#[derive(Debug, Clone)]
pub struct ExecutionStore {
    path: PathBuf,
}

impl ExecutionStore {
    /// The store for an explicit state root and project path.
    pub fn for_project(state_root: impl AsRef<Path>, project: &str) -> Self {
        Self {
            path: state_root.as_ref().join(format!("{}.json", slug(project))),
        }
    }

    /// The store for `project` under the state root of `paths`.
    pub fn open(paths: &Paths, project: &Path) -> Self {
        Self::for_project(&paths.state_root, &project.to_string_lossy())
    }

    /// The store for the context's state root and project.
    pub fn open_in(ctx: &crate::runtime::RuntimeContext) -> Result<Self> {
        let project = ctx.paths.project()?.to_string_lossy().into_owned();
        Ok(Self::for_project(&ctx.paths.state_root, &project))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// All records, oldest write order preserved. An absent or empty file reads
    /// as an empty ledger, matching the bash `[]` default.
    pub fn read(&self) -> Result<Vec<LedgerRecordV1>> {
        let Ok(raw) = std::fs::read_to_string(&self.path) else {
            return Ok(Vec::new());
        };
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

    /// Replace the file with `records`, durably. A record's `status` is
    /// brought in line with its `state` first.
    fn write(&self, records: &mut [LedgerRecordV1]) -> Result<()> {
        for r in records.iter_mut() {
            r.sync_legacy_status();
        }
        let json = Self::render_json(records)?;
        fsx::write_atomic(&self.path, json.as_bytes(), LEDGER_MODE)
            .with_context(|| format!("replacing {}", self.path.display()))
    }

    /// Take the lock, mutate the records, write them back.
    pub fn update<T>(&self, f: impl FnOnce(&mut Vec<LedgerRecordV1>) -> Result<T>) -> Result<T> {
        let root = self
            .path
            .parent()
            .context("ledger path has no parent directory")?;
        let name = self
            .path
            .file_name()
            .context("ledger path has no file name")?
            .to_string_lossy();
        let _guard = DirLock::acquire(root, &name, LOCK_STALE_AFTER, LOCK_TIMEOUT)
            .with_context(|| format!("locking ledger {}", self.path.display()))?;
        let mut records = self.read()?;
        let out = f(&mut records)?;
        self.write(&mut records)?;
        Ok(out)
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
                bail!("no record matches '{key}' in {}", self.path.display())
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
            records.push(record);
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
    pub fn mark_starting(&self, key: &str, pane: &str) -> Result<()> {
        let at = crate::clock::now_stamp();
        self.update_key(key, |r| {
            r.pane_id = Some(pane.to_string());
            r.finished_at = None;
            r.set_state(ExecutionStatus::Starting);
            r.updated_at = at.clone();
        })
    }

    /// The agent process ended with `code` (`None`: killed by a signal, or it
    /// never started). A live record ends: `Done` on 0, and
    /// `Failed(AgentExited)` otherwise, unless it is an orchestrator, which
    /// keeps its state. A record that is already terminal (the worker ran
    /// `horch done`) only gets the code.
    pub fn record_exit(&self, key: &str, code: Option<i32>) -> Result<()> {
        let at = crate::clock::now_stamp();
        self.update_key(key, |r| {
            r.exit_code = code;
            r.updated_at = at.clone();
            if r.is_orchestrator() || !r.execution_status().is_live() {
                return;
            }
            let state = match code {
                Some(0) => ExecutionStatus::Done,
                code => ExecutionStatus::Failed {
                    failure: FailureKind::AgentExited { code },
                },
            };
            r.finished_at.get_or_insert_with(|| at.clone());
            r.set_state(state);
        })
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
        if !self.read()?.iter().any(abandoned) {
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

    /// Records whose typed status is live: starting or running.
    pub fn live(&self) -> Result<Vec<LedgerRecordV1>> {
        Ok(self
            .read()?
            .into_iter()
            .filter(|r| r.execution_status().is_live())
            .collect())
    }
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
        out.push_str(&format!("  task: {}\n", r.task));
        let notable: Vec<&crate::execution::legacy::HistoryEntry> = r
            .history
            .iter()
            .filter(|h| h.event == "done" || h.event == "note")
            .collect();
        for h in notable.iter().rev().take(3).rev() {
            out.push_str(&format!("  {} @ {}: {}\n", h.event, h.at, h.text));
        }
        out.push('\n');
    }
}
