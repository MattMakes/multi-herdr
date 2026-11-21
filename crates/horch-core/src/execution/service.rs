//! The spawn workflow: apply an [`ExecutionPlan`] in a fixed order.
//!
//! insert the record (`Planned`) → write the brief → split a pane → run the
//! worker in it → mark the record `Starting` with its pane → tile, best
//! effort. A step that fails leaves no live-looking record: a failed split
//! records `LaunchFailed{Split}` and frees the role; a failed run also closes
//! the pane. The record is never live before its pane runs the worker.
//!
//! `HORCH_FAULT` points: `abort-after-execution-insert`, `abort-after-brief`,
//! `abort-after-pane-split` (the process ends there, as in a crash) and
//! `fail-pane-split`, `fail-pane-run` (the step fails).

use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::execution::legacy::LedgerRecordV1;
use crate::execution::model::{ExecutionPlan, ExecutionStatus, LaunchStage, TilingMode};
use crate::execution::plan::{finish_plan, PlanError};
use crate::execution::store::{from_execution, ExecutionStore};
use crate::ids::{ExecutionId, RoleName, WorkspaceId};
use crate::messaging::brief::{Brief, SCHEMA};
use crate::messaging::mailbox::Mailbox;
use crate::routing::decision::RoutingDecision;
use crate::runtime::RuntimeContext;
use crate::workspace::client::WorkspaceClient;
use crate::workspace::paneshell::PaneShell;

const ROLE_TRIES: usize = 64;
const CLAIM_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(60);
const CLAIM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Why a spawn did not start a worker.
#[derive(Debug)]
pub enum SpawnError {
    /// The usage-limit gate refused. The CLI prints `line` and exits 3.
    Refused {
        decision: RoutingDecision,
        line: String,
    },
    /// Nothing was written.
    Plan(PlanError),
    /// The record exists, now `LaunchFailed{stage}`, and its role is free.
    LaunchFailed {
        id: ExecutionId,
        stage: LaunchStage,
        source: anyhow::Error,
    },
    /// The ledger or the mailbox could not be written before the launch.
    Store(anyhow::Error),
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { .. } => f.write_str("spawn refused over usage limits"),
            Self::Plan(e) => write!(f, "{e}"),
            Self::LaunchFailed { id, stage, source } => {
                let stage = match stage {
                    LaunchStage::Brief => "writing the brief",
                    LaunchStage::Split => "splitting a pane",
                    LaunchStage::Run => "running the worker",
                };
                write!(f, "launch of {id} failed {stage}: {source:#}")
            }
            Self::Store(e) => write!(f, "{e:#}"),
        }
    }
}

// No `source`: `Display` already carries the cause, and `{e:#}` would
// print it twice.
impl std::error::Error for SpawnError {}

impl From<PlanError> for SpawnError {
    fn from(e: PlanError) -> Self {
        match e {
            PlanError::Refused { decision, line } => Self::Refused { decision, line },
            e => Self::Plan(e),
        }
    }
}

/// A worker that started: its finished plan and its pane.
#[derive(Debug, Clone)]
pub struct SpawnOutcome {
    pub plan: ExecutionPlan,
    pub pane: String,
}

/// Custom pane-creation callback for hosts without geometry.
pub type OpenPane<'a> = &'a dyn Fn(&ExecutionPlan) -> Result<String>;

/// Applies spawn plans in one project and workspace.
pub struct ExecutionService<'a> {
    pub ctx: &'a RuntimeContext,
    pub store: &'a ExecutionStore,
    pub workspace: &'a dyn WorkspaceClient,
    /// The workspace's mailbox: roles and briefs.
    pub mailbox: &'a Mailbox,
    /// Lay the grid out around a new pane. Called last; must not fail.
    pub tile: &'a dyn Fn(&str, TilingMode),
    /// Open a worker pane on a host without geometry (`Caps { geometry: false }`)
    /// when `plan.workspace.from_pane` is `None`.
    pub open_pane: Option<OpenPane<'a>>,
    /// The worker's task ledger row, written into its brief.
    pub ledger_worker: Option<&'a str>,
    /// `task.yaml` of the worker's push attempt, written into its brief.
    pub task_yaml: Option<&'a str>,
}

impl ExecutionService<'_> {
    /// Start the worker `plan` describes. `role` is an explicit role; `None`
    /// allocates `<teammate>-<n>`.
    pub fn spawn(
        &self,
        plan: ExecutionPlan,
        role: Option<&str>,
    ) -> Result<SpawnOutcome, SpawnError> {
        self.recover_abandoned();
        let faults = &self.ctx.settings.faults;

        let plan = self.insert(plan, role)?;
        let id = plan.execution.id.clone();
        let role = plan.execution.role.to_string();
        // SKL-04: the exact skill versions this execution is briefed with.
        self.store
            .set_skills(id.as_str(), plan.skills.activated.clone())
            .map_err(SpawnError::Store)?;
        faults.abort_if("abort-after-execution-insert");

        let pane = self
            .launch_stages(&plan, &role)
            .map_err(|(stage, e, pane)| self.launch_failed(&plan, stage, e, pane.as_deref()))?;
        // The worker is running in its pane; a ledger hiccup here must not
        // fail the spawn. The worker sets `Running` itself.
        if let Err(e) = self.store.mark_starting(id.as_str(), &pane) {
            eprintln!("horch spawn: recording pane {pane} for {id} failed: {e:#}");
        }
        (self.tile)(&pane, plan.workspace.tiling);
        Ok(SpawnOutcome { plan, pane })
    }

    /// Brief, split and run, with the crash fault points between them. A
    /// failure names its stage and the pane to close, if one exists.
    fn launch_stages(
        &self,
        plan: &ExecutionPlan,
        role: &str,
    ) -> Result<String, (LaunchStage, anyhow::Error, Option<String>)> {
        let faults = &self.ctx.settings.faults;
        self.write_brief(plan)
            .map_err(|e| (LaunchStage::Brief, e, None))?;
        faults.abort_if("abort-after-brief");

        let pane = self
            .split(plan)
            .map_err(|e| (LaunchStage::Split, e, None))?;
        faults.abort_if("abort-after-pane-split");

        self.run(&pane, role)
            .map_err(|e| (LaunchStage::Run, e, Some(pane.clone())))?;
        Ok(pane)
    }

    /// Close what earlier spawners abandoned, and free their roles. Best
    /// effort: a spawn never fails over another spawn's leftovers.
    fn recover_abandoned(&self) {
        match self.store.recover_abandoned(crate::clock::now()) {
            Ok(closed) => {
                for r in closed {
                    if r.workspace_id.as_deref() == Some(self.mailbox.workspace_id()) {
                        self.mailbox.unregister(&r.role);
                    }
                }
            }
            Err(e) => eprintln!("horch spawn: recovering abandoned spawns failed: {e:#}"),
        }
    }

    fn insert(&self, plan: ExecutionPlan, role: Option<&str>) -> Result<ExecutionPlan, SpawnError> {
        let workspace = WorkspaceId::new(self.mailbox.workspace_id())
            .map_err(|e| SpawnError::Store(e.into()))?;
        let _claim = self.claim_lock().map_err(SpawnError::Store)?;
        let outcome = self
            .store
            .update(|records| self.claim(records, plan.clone(), role, &workspace));
        match outcome {
            Ok(Ok(plan)) => Ok(plan),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(SpawnError::Store(e)),
        }
    }

    fn claim_lock(&self) -> Result<crate::fsx::DirLockGuard> {
        let dir = self.mailbox.dir();
        let parent = dir
            .parent()
            .ok_or_else(|| anyhow!("the mailbox {} has no parent folder", dir.display()))?;
        let name = format!("role-claim-{}", self.mailbox.workspace_id());
        Ok(crate::fsx::DirLock::acquire(
            parent,
            &name,
            CLAIM_STALE_AFTER,
            CLAIM_TIMEOUT,
        )?)
    }

    fn claim(
        &self,
        records: &mut Vec<LedgerRecordV1>,
        plan: ExecutionPlan,
        role: Option<&str>,
        workspace: &WorkspaceId,
    ) -> Result<Result<ExecutionPlan, PlanError>> {
        let taken =
            |role: &str| self.mailbox.role_taken(role) || planned_role(records, workspace, role);
        let role = match role {
            Some(role) if taken(role) => return Ok(Err(PlanError::RoleTaken(role.to_string()))),
            Some(role) => role.to_string(),
            None => {
                let teammate = &plan.launch.teammate.name;
                let mut role = String::new();
                for _ in 0..ROLE_TRIES {
                    role = format!("{teammate}-{}", self.mailbox.next_seq(teammate)?);
                    if !taken(&role) {
                        break;
                    }
                }
                if taken(&role) {
                    return Ok(Err(PlanError::RoleTaken(role)));
                }
                role
            }
        };
        let mut plan = finish_plan(plan, RoleName::new(role)?, workspace);
        plan.execution.backend = self.ctx.herdr.backend;
        upsert(records, from_execution(&plan.execution), plan.resumed)?;
        Ok(Ok(plan))
    }

    /// Build the brief and write it to the mailbox.
    fn write_brief(&self, plan: &ExecutionPlan) -> Result<()> {
        let ctx = self.ctx;
        let text = |p: Option<&Path>| {
            p.map(|p| p.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
        };
        let launch = &plan.launch;
        let e = &plan.execution;
        // A spawned pane is a fresh shell started by the herdr server, so it
        // inherits the user's profile, not this process's environment. The
        // overrides travel in the brief, or `HORCH_CLAUDE_BIN=claude horch
        // fleet` would have no effect on the workers it spawns.
        let mut brief = Brief {
            schema: SCHEMA,
            role: e.role.to_string(),
            teammate: launch.teammate.name.clone(),
            agent: launch.teammate.agent.as_str().to_string(),
            model: launch.model.clone(),
            record_id: e.id.to_string(),
            session: launch.session.clone(),
            task: launch.task.clone(),
            project_dir: ctx.paths.project()?.to_string_lossy().into_owned(),
            state_dir: text(ctx.paths.state_override.as_deref()),
            claude_bin: None,
            codex_bin: None,
            resolved: Some(launch.teammate.clone()),
            teammates_dir: text(ctx.bins.roster_override.as_deref()),
            workdir: text(e.workdir.as_deref()),
            bin_overrides: Default::default(),
            report_to: plan.report_to,
            worker_id: None,
            ledger_db: None,
            // A relative `FLEET_ROOT` is relative to the spawner's cwd, so
            // the brief carries it absolute.
            fleet_root: Some(
                std::path::absolute(&ctx.paths.fleet_root)?
                    .to_string_lossy()
                    .into_owned(),
            ),
            controller: plan.controller.clone(),
            task_yaml: None,
            backend: ctx.herdr.backend,
        };
        brief.set_overrides(ctx.bins.overrides.clone());
        brief.worker_id = self.ledger_worker.map(str::to_string);
        brief.task_yaml = self.task_yaml.map(str::to_string);
        brief.ledger_db = text(ctx.paths.ledger_override.as_deref());
        self.mailbox.write_brief(&brief)
    }

    /// Split the requested pane, or this process's own.
    fn split(&self, plan: &ExecutionPlan) -> Result<String> {
        if plan.workspace.from_pane.is_none() {
            if let Some(open) = self.open_pane {
                if self.ctx.settings.faults.has("fail-pane-split") {
                    return Err(anyhow!("HORCH_FAULT fail-pane-split"));
                }
                return open(plan);
            }
        }
        let from = match &plan.workspace.from_pane {
            Some(p) => p.clone(),
            None => {
                let internal = self.ctx.herdr.pane.as_ref().context(
                    "horch spawn needs --from-pane when not inside a herdr pane or a Herdr app terminal (HERDR_PANE_ID and HC_TERMINAL_ID are unset)",
                )?;
                self.workspace.pane_get(internal.as_ref())?.pane_id
            }
        };
        if self.ctx.settings.faults.has("fail-pane-split") {
            return Err(anyhow!("HORCH_FAULT fail-pane-split"));
        }
        self.workspace.pane_split(&from, plan.workspace.direction)
    }

    fn run(&self, pane: &str, role: &str) -> Result<()> {
        if self.ctx.settings.faults.has("fail-pane-run") {
            return Err(anyhow!("HORCH_FAULT fail-pane-run"));
        }
        let exe = self.ctx.bins.exe()?;
        let paths = &self.ctx.paths;
        let mut vars = vec![
            ("HORCH_DATA_DIR", paths.data_root.to_string_lossy()),
            ("HORCH_STATE_DIR", paths.state_root.to_string_lossy()),
            ("HORCH_LEDGER_DB", paths.ledger_db.to_string_lossy()),
        ];
        if self.ctx.herdr.backend == crate::runtime::Backend::Hc {
            vars.push(("HORCH_BACKEND", "hc".into()));
        }
        let vars: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (*k, v.as_ref())).collect();
        let command = PaneShell::host().command_line_with_env(&exe, &vars, &["worker", role]);
        self.workspace.pane_run(pane, &command)
    }

    /// Record the failure, close the pane if one exists, and free the role.
    /// Cleanup errors are logged: the launch error is the one to report.
    fn launch_failed(
        &self,
        plan: &ExecutionPlan,
        stage: LaunchStage,
        source: anyhow::Error,
        pane: Option<&str>,
    ) -> SpawnError {
        let id = plan.execution.id.clone();
        let state = ExecutionStatus::LaunchFailed {
            stage,
            reason: format!("{source:#}"),
        };
        if let Err(e) = self.store.end_live(id.as_str(), state) {
            eprintln!("horch spawn: recording the failed launch of {id} failed: {e:#}");
        }
        if let Some(pane) = pane {
            if let Err(e) = self.workspace.pane_close(pane) {
                eprintln!("horch spawn: closing pane {pane} failed: {e:#}");
            }
        }
        self.mailbox.unregister(plan.execution.role.as_str());
        SpawnError::LaunchFailed { id, stage, source }
    }
}

/// Whether a `Planned` record in this workspace holds `role`: its spawner
/// has not briefed it yet, so the mailbox does not show it.
fn planned_role(records: &[LedgerRecordV1], workspace: &WorkspaceId, role: &str) -> bool {
    records.iter().any(|r| {
        r.role == role
            && r.workspace_id.as_deref() == Some(workspace.as_str())
            && r.state == Some(ExecutionStatus::Planned)
    })
}

/// Write `record` into the ledger rows. A resume replaces its row in place,
/// keeping any history written since the plan read it; a fresh spawn pushes.
fn upsert(records: &mut Vec<LedgerRecordV1>, record: LedgerRecordV1, resumed: bool) -> Result<()> {
    match records.iter_mut().find(|r| r.record_id == record.record_id) {
        Some(old) if resumed => {
            let mut history = old.history.clone();
            history.extend(record.history.last().cloned());
            *old = LedgerRecordV1 { history, ..record };
        }
        Some(_) => anyhow::bail!("record {} exists already", record.record_id),
        None if resumed => {
            anyhow::bail!("record {} vanished before the resume", record.record_id)
        }
        None => records.push(record),
    }
    Ok(())
}
