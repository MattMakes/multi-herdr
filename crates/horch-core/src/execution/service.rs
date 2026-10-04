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

/// Applies spawn plans in one project and workspace.
pub struct ExecutionService<'a> {
    pub ctx: &'a RuntimeContext,
    pub store: &'a ExecutionStore,
    pub workspace: &'a dyn WorkspaceClient,
    /// The workspace's mailbox: roles and briefs.
    pub mailbox: &'a Mailbox,
    /// Lay the grid out around a new pane. Called last; must not fail.
    pub tile: &'a dyn Fn(&str, TilingMode),
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

        if let Err(e) = self.write_brief(&plan) {
            return Err(self.launch_failed(&plan, LaunchStage::Brief, e, None));
        }
        faults.abort_if("abort-after-brief");

        let pane = match self.split(&plan) {
            Ok(pane) => pane,
            Err(e) => return Err(self.launch_failed(&plan, LaunchStage::Split, e, None)),
        };
        faults.abort_if("abort-after-pane-split");

        if let Err(e) = self.run(&pane, &role) {
            return Err(self.launch_failed(&plan, LaunchStage::Run, e, Some(&pane)));
        }
        // The worker is running in its pane; a ledger hiccup here must not
        // fail the spawn. The worker sets `Running` itself.
        if let Err(e) = self.store.mark_starting(id.as_str(), &pane) {
            eprintln!("horch spawn: recording pane {pane} for {id} failed: {e:#}");
        }
        (self.tile)(&pane, plan.workspace.tiling);
        Ok(SpawnOutcome { plan, pane })
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

    /// Allocate the role and write the `Planned` record, under the ledger
    /// lock, so two spawns cannot take one role.
    fn insert(&self, plan: ExecutionPlan, role: Option<&str>) -> Result<ExecutionPlan, SpawnError> {
        let workspace = WorkspaceId::new(self.mailbox.workspace_id())
            .map_err(|e| SpawnError::Store(e.into()))?;
        let outcome = self.store.update(|records| {
            let role = match role {
                Some(role) => role.to_string(),
                None => {
                    let name = &plan.launch.teammate.name;
                    format!("{name}-{}", self.mailbox.next_seq(name)?)
                }
            };
            if self.mailbox.role_taken(&role) || planned_role(records, &workspace, &role) {
                return Ok(Err(PlanError::RoleTaken(role)));
            }
            let plan = finish_plan(plan, RoleName::new(role)?, &workspace);
            let record = from_execution(&plan.execution);
            match records.iter_mut().find(|r| r.record_id == record.record_id) {
                // A resume: the record as planned, with any history written
                // since the plan read it.
                Some(old) if plan.resumed => {
                    let mut history = old.history.clone();
                    history.extend(plan.execution.history.last().cloned());
                    *old = LedgerRecordV1 { history, ..record };
                }
                Some(_) => anyhow::bail!("record {} exists already", record.record_id),
                None if plan.resumed => {
                    anyhow::bail!("record {} vanished before the resume", record.record_id)
                }
                None => records.push(record),
            }
            Ok(Ok(plan))
        });
        match outcome {
            Ok(Ok(plan)) => Ok(plan),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(SpawnError::Store(e)),
        }
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
        };
        brief.set_overrides(ctx.bins.overrides.clone());
        self.mailbox.write_brief(&brief)
    }

    /// Split the requested pane, or this process's own.
    fn split(&self, plan: &ExecutionPlan) -> Result<String> {
        let from = match &plan.workspace.from_pane {
            Some(p) => p.clone(),
            None => {
                let internal =
                    self.ctx.herdr.pane.as_ref().context(
                        "horch spawn needs --from-pane when not run inside a herdr pane",
                    )?;
                self.workspace.pane_get(internal.as_str())?.pane_id
            }
        };
        if self.ctx.settings.faults.has("fail-pane-split") {
            return Err(anyhow!("HORCH_FAULT fail-pane-split"));
        }
        self.workspace.pane_split(&from, plan.workspace.direction)
    }

    /// Run `horch worker <role>` in the new pane.
    fn run(&self, pane: &str, role: &str) -> Result<()> {
        if self.ctx.settings.faults.has("fail-pane-run") {
            return Err(anyhow!("HORCH_FAULT fail-pane-run"));
        }
        let exe = self.ctx.bins.exe()?;
        let command = PaneShell::host().command_line(&exe, &["worker", role]);
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
        if let Err(e) = self.store.set_state(id.as_str(), state) {
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
