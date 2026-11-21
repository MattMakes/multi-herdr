use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use chrono::{DateTime, Utc};

use crate::execution::legacy::{
    HistoryEntry, LedgerRecordV1, LedgerWindow, KIND_ORCHESTRATOR, KIND_WORKER, STATUS_WORKING,
};
use crate::execution::model::{
    Execution, ExecutionKind, ExecutionStatus, FailureKind, LaunchStage, SessionState, Task,
};
use crate::ids::{IdError, PaneId, WorkspaceId};
use crate::runtime::Paths;
use crate::skills::activation::ResolvedSkillRef;
use crate::workspace::client::WorkspaceClient;

pub const PANE_CLOSED: &str = "pane closed without horch done";

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

pub(crate) fn read_ledger_file(path: &Path) -> Option<Vec<LedgerRecordV1>> {
    let text = std::fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        return Some(Vec::new());
    }
    serde_json::from_str(&text).ok()
}

pub fn read_all_ledgers(state_root: &Path) -> Vec<(PathBuf, Vec<LedgerRecordV1>)> {
    ledger_paths(state_root)
        .into_iter()
        .filter_map(|p| read_ledger_file(&p).map(|records| (p, records)))
        .collect()
}

const JUDGE_LABEL: &str = "judge:";

pub const ABANDONED_AFTER: chrono::Duration = chrono::Duration::minutes(5);

#[derive(Debug)]
pub enum LegacyError {
    Id(IdError),
    Agent(String),
    Kind { record: String, reason: String },
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
        backend: r.backend,
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
        backend: e.backend,
    }
}

pub const STATE_DB: &str = "ledger.sqlite3";

pub fn session_db(paths: &Paths) -> PathBuf {
    if paths.state_root == paths.default_state_root {
        paths.ledger_db.clone()
    } else {
        paths.state_root.join(STATE_DB)
    }
}

pub fn session_files(state_root: &Path) -> (PathBuf, Option<PathBuf>) {
    let paths = Paths::from_env(&crate::runtime::ProcessEnv);
    if paths.state_root == state_root {
        (session_db(&paths), Some(paths.ledger_db))
    } else {
        (state_root.join(STATE_DB), None)
    }
}

struct SessionRow {
    project_root: String,
    slug: String,
    record: LedgerRecordV1,
}

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
            })
        })
        .collect())
}

fn read_session_rows(db: &Path, slug: Option<&str>) -> Vec<SessionRow> {
    if !db.is_file() {
        return Vec::new();
    }
    crate::taskledger::Store::open(db)
        .and_then(|store| session_rows(store.conn(), slug))
        .unwrap_or_default()
}

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

pub struct ProjectRecords {
    pub path: PathBuf,
    pub project: Option<String>,
    pub records: Vec<LedgerRecordV1>,
}

pub fn read_all_projects(
    state_root: &Path,
    db: &Path,
    task_ledger: Option<&Path>,
) -> Vec<ProjectRecords> {
    merge_projects(state_root, read_all_ledgers(state_root), db, task_ledger)
}

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

#[derive(Debug, Clone)]
pub struct ExecutionStore {
    path: PathBuf,
    db: PathBuf,
    project: String,
    slug: String,
    task_ledger: Option<(PathBuf, PathBuf)>,
}

impl ExecutionStore {
    pub fn for_project(state_root: impl AsRef<Path>, project: &str) -> Self {
        let state_root = state_root.as_ref();
        Self::in_db(state_root, &state_root.join(STATE_DB), project)
    }

    pub fn in_db(state_root: &Path, db: &Path, project: &str) -> Self {
        let slug = slug(project);
        Self {
            path: state_root.join(format!("{slug}.json")),
            db: db.to_path_buf(),
            project: project.to_string(),
            slug,
            task_ledger: None,
        }
    }

    pub fn open(paths: &Paths, project: &Path) -> Self {
        Self::in_db(
            &paths.state_root,
            &session_db(paths),
            &project.to_string_lossy(),
        )
        .with_task_ledger(&paths.ledger_db, project)
    }

    pub fn open_in(ctx: &crate::runtime::RuntimeContext) -> Result<Self> {
        Ok(Self::open(&ctx.paths, &ctx.paths.project()?))
    }

    pub fn with_task_ledger(mut self, db: &Path, project: &Path) -> Self {
        self.task_ledger = Some((db.to_path_buf(), project.to_path_buf()));
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn db(&self) -> &Path {
        &self.db
    }

    pub fn read(&self) -> Result<Vec<LedgerRecordV1>> {
        let mut records = self.read_file()?;
        if let Some((db, project)) = &self.task_ledger {
            overlay_task_ledger(&mut records, db, project);
        }
        Ok(records)
    }

    fn read_file(&self) -> Result<Vec<LedgerRecordV1>> {
        let rows = read_session_rows(&self.db, Some(&self.slug));
        Ok(merge_legacy(
            self.read_legacy()?,
            rows.into_iter().map(|r| r.record).collect(),
        ))
    }

    pub fn stored_json(&self) -> Result<String> {
        Self::render_json(&self.read_file()?)
    }

    fn read_legacy(&self) -> Result<Vec<LedgerRecordV1>> {
        let raw = std::fs::read_to_string(&self.path).unwrap_or_default();
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing ledger {}", self.path.display()))
    }

    pub fn render_json(records: &[LedgerRecordV1]) -> Result<String> {
        Ok(serde_json::to_string_pretty(records)?)
    }

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

    fn write(&self, tx: &rusqlite::Transaction, records: &mut [LedgerRecordV1]) -> Result<()> {
        tx.execute("DELETE FROM session_records WHERE slug = ?1", [&self.slug])?;
        let mut insert = tx.prepare(
            "INSERT OR REPLACE INTO session_records
                 (record_id, worker_id, project_root, slug, record_json, updated_at)
             VALUES (?1, (SELECT worker_id FROM workers
                          WHERE worker_id = ?1 OR legacy_record_id = ?1
                          ORDER BY rowid DESC LIMIT 1),
                     ?2, ?3, ?4, ?5)",
        )?;
        for r in records.iter_mut() {
            r.sync_legacy_status();
            insert.execute(rusqlite::params![
                r.record_id,
                self.project,
                self.slug,
                serde_json::to_string(r)?,
                r.updated_at,
            ])?;
        }
        Ok(())
    }

    pub fn update<T>(&self, f: impl FnOnce(&mut Vec<LedgerRecordV1>) -> Result<T>) -> Result<T> {
        if let Some(dir) = self.db.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut store = crate::taskledger::Store::open(&self.db)
            .with_context(|| format!("opening the session records in {}", self.db.display()))?;
        store.immediate(|tx| {
            let rows = session_rows(tx, Some(&self.slug))?;
            let mut records = merge_legacy(
                self.read_legacy()?,
                rows.into_iter().map(|r| r.record).collect(),
            );
            let out = f(&mut records)?;
            self.write(tx, &mut records)?;
            Ok(out)
        })
    }

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

    pub fn insert(&self, record: LedgerRecordV1) -> Result<()> {
        self.update(|records| {
            records.push(record);
            Ok(())
        })
    }

    pub fn get(&self, key: &str) -> Result<LedgerRecordV1> {
        self.read()?
            .into_iter()
            .rfind(|r| r.matches(key))
            .with_context(|| format!("no record matches {key}"))
    }

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

    pub fn set_skills(&self, key: &str, skills: Vec<ResolvedSkillRef>) -> Result<()> {
        self.update_key(key, |r| r.skills = skills.clone())
    }

    pub fn set_compact_window(&self, key: &str, window: &LedgerWindow) -> Result<()> {
        self.update_key(key, |r| r.compact_window = Some(window.clone()))
    }

    pub fn load(&self) -> Result<Vec<Execution>> {
        self.read()?
            .iter()
            .map(|r| to_execution(r).map_err(anyhow::Error::from))
            .collect()
    }

    pub fn insert_execution(&self, e: &Execution) -> Result<()> {
        self.insert(from_execution(e))
    }

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

    pub fn record_exit(&self, key: &str, code: Option<i32>) -> Result<bool> {
        let at = crate::clock::now_stamp();
        let mut ended = false;
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
            ended = true;
        })?;
        Ok(ended)
    }

    pub fn recover_abandoned(&self, now: DateTime<Utc>) -> Result<Vec<LedgerRecordV1>> {
        let abandoned = |r: &LedgerRecordV1| {
            r.state == Some(ExecutionStatus::Planned)
                && r.pane_id.is_none()
                && crate::clock::parse(&r.updated_at).is_some_and(|t| now - t >= ABANDONED_AFTER)
        };
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

    pub fn end_if_pane_closed(&self, key: &str, ws: &dyn WorkspaceClient) -> Result<bool> {
        let at = crate::clock::now_stamp();
        match self.closed_working_record(key, ws) {
            None => Ok(false),
            Some(record) => self.update(|rs| Ok(mark_pane_vanished(rs, &record.record_id, &at))),
        }
    }

    fn closed_working_record(&self, key: &str, ws: &dyn WorkspaceClient) -> Option<LedgerRecordV1> {
        let record = self.get(key).ok()?;
        if record.status != STATUS_WORKING || record.is_orchestrator() {
            return None;
        }
        let pane = record.pane_id.as_deref()?;
        let gone = ws.pane_get(pane).is_err() && ws.server_reachable();
        gone.then_some(record)
    }

    pub fn live(&self) -> Result<Vec<LedgerRecordV1>> {
        Ok(self
            .read()?
            .into_iter()
            .filter(|r| r.execution_status().is_live())
            .collect())
    }

    pub fn backend_of_workspace(&self, workspace: &str) -> Result<Option<crate::runtime::Backend>> {
        Ok(self
            .read()?
            .into_iter()
            .rev()
            .find(|r| r.workspace_id.as_deref() == Some(workspace))
            .map(|r| r.backend))
    }
}

pub fn workspace_backend(paths: &Paths, workspace: &str) -> Option<crate::runtime::Backend> {
    read_all_projects(
        &paths.state_root,
        &session_db(paths),
        Some(&paths.ledger_db),
    )
    .into_iter()
    .flat_map(|p| p.records.into_iter())
    .rfind(|r| r.workspace_id.as_deref() == Some(workspace))
    .map(|r| r.backend)
}

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

struct LedgerWorker {
    worker: crate::taskledger::workers::Worker,
    task: Option<String>,
}

fn ledger_workers(db: &Path, project: &Path) -> Vec<LedgerWorker> {
    if !db.is_file() {
        return Vec::new();
    }
    let read = || -> Result<Vec<LedgerWorker>> {
        let store = crate::taskledger::Store::open(db)?;
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
    if w.backend != crate::runtime::Backend::Herdr {
        r.backend = w.backend;
    }
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
        backend: w.backend,
        ..LedgerRecordV1::default()
    };
    r.set_state(worker_state(w));
    overlay_record(&mut r, lw);
    r
}

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
        let ledger =
            SessionLedger::for_project(tmp.path().join("state"), project.to_str().unwrap())
                .with_task_ledger(&db, &project);
        Fixture {
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

    #[test]
    fn trf_45_every_session_write_goes_to_the_table_and_no_json_file() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
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

        store.mark_running("old-1").unwrap();
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), text);
        let rows = read_session_rows(store.db(), Some("-repo"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].record.execution_status(), ExecutionStatus::Running);
        assert_eq!(store.read().unwrap().len(), 1, "no duplicate");
    }

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
        let moved = at(MapEnv::new("/")
            .with("HORCH_STATE_DIR", "/tmp/s")
            .with("HORCH_LEDGER_DB", "/tmp/l.sqlite3"));
        assert_eq!(session_db(&moved), Path::new("/tmp/s/ledger.sqlite3"));
    }

    #[test]
    fn hcb_03_a_record_without_backend_reads_as_herdr() {
        let raw = r#"{
  "record_id": "r-herdr",
  "session_id": "s-1",
  "agent": "claude",
  "tier": "sonnet",
  "model": "sonnet",
  "role": "sonnet-1",
  "status": "working",
  "task": "t",
  "history": [],
  "created_at": "2026-11-19T00:00:00Z",
  "updated_at": "2026-11-19T00:00:00Z",
  "workspace_id": "w1",
  "pane_id": "w1:p1"
}"#;
        let record: LedgerRecordV1 = serde_json::from_str(raw).unwrap();
        assert_eq!(record.backend, crate::runtime::Backend::Herdr);
        let exec = to_execution(&record).unwrap();
        assert_eq!(exec.backend, crate::runtime::Backend::Herdr);
    }

    #[test]
    fn hcb_03_a_herdr_record_keeps_its_bytes() {
        let raw = r#"[
  {
    "record_id": "r-herdr",
    "session_id": "s-1",
    "agent": "claude",
    "tier": "sonnet",
    "model": "sonnet",
    "role": "sonnet-1",
    "status": "working",
    "task": "t",
    "history": [],
    "created_at": "2026-11-19T00:00:00Z",
    "updated_at": "2026-11-19T00:00:00Z",
    "workspace_id": "w1",
    "pane_id": "w1:p1"
  }
]"#;
        let records: Vec<LedgerRecordV1> = serde_json::from_str(raw).unwrap();
        let exec = to_execution(&records[0]).unwrap();
        let round_tripped = from_execution(&exec);
        let rendered = ExecutionStore::render_json(&[round_tripped]).unwrap();
        assert_eq!(rendered, raw);
        assert!(!rendered.contains("backend"));

        let mut hc_exec = exec;
        hc_exec.backend = crate::runtime::Backend::Hc;
        let hc_record = from_execution(&hc_exec);
        let hc_json = serde_json::to_value(&hc_record).unwrap();
        assert_eq!(hc_json["backend"], "hc");
        let back: LedgerRecordV1 = serde_json::from_value(hc_json).unwrap();
        assert_eq!(
            to_execution(&back).unwrap().backend,
            crate::runtime::Backend::Hc
        );
    }

    #[test]
    fn hcb_03_backend_of_workspace_finds_the_owner() {
        use crate::taskledger::runs::{self, NewRun};
        use crate::taskledger::workers::{self, NewWorker};
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("ledger.sqlite3");
        let mut ledger = crate::taskledger::Store::open(&db).unwrap();
        ledger
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        workspace_id: Some("sp-row".into()),
                        ..Default::default()
                    },
                )?;
                workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id,
                        persona: "opus".into(),
                        workspace_id: Some("sp-row".into()),
                        pane_id: Some("t3".into()),
                        backend: crate::runtime::Backend::Hc,
                        ..NewWorker::default()
                    },
                )
            })
            .unwrap();

        let store = ExecutionStore::in_db(tmp.path(), &db, "/repo")
            .with_task_ledger(&db, Path::new("/repo"));
        store
            .insert(LedgerRecordV1 {
                record_id: "r-herdr".into(),
                role: "sonnet-1".into(),
                status: STATUS_WORKING.into(),
                workspace_id: Some("ws-herdr".into()),
                backend: crate::runtime::Backend::Herdr,
                ..LedgerRecordV1::default()
            })
            .unwrap();
        store
            .insert(LedgerRecordV1 {
                record_id: "r-hc".into(),
                role: "sonnet-2".into(),
                status: STATUS_WORKING.into(),
                workspace_id: Some("sp-rec".into()),
                backend: crate::runtime::Backend::Hc,
                ..LedgerRecordV1::default()
            })
            .unwrap();

        assert_eq!(
            store.backend_of_workspace("ws-herdr").unwrap(),
            Some(crate::runtime::Backend::Herdr)
        );
        assert_eq!(
            store.backend_of_workspace("sp-rec").unwrap(),
            Some(crate::runtime::Backend::Hc)
        );
        assert_eq!(
            store.backend_of_workspace("sp-row").unwrap(),
            Some(crate::runtime::Backend::Hc)
        );
        assert_eq!(store.backend_of_workspace("unknown").unwrap(), None);
    }
}
