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

#[derive(Debug)]
pub enum SpawnError {
    Refused {
        decision: RoutingDecision,
        line: String,
    },
    Plan(PlanError),
    LaunchFailed {
        id: ExecutionId,
        stage: LaunchStage,
        source: anyhow::Error,
    },
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

impl std::error::Error for SpawnError {}

impl From<PlanError> for SpawnError {
    fn from(e: PlanError) -> Self {
        match e {
            PlanError::Refused { decision, line } => Self::Refused { decision, line },
            e => Self::Plan(e),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpawnOutcome {
    pub plan: ExecutionPlan,
    pub pane: String,
}

pub type OpenPane<'a> = &'a dyn Fn(&ExecutionPlan) -> Result<String>;

pub struct ExecutionService<'a> {
    pub ctx: &'a RuntimeContext,
    pub store: &'a ExecutionStore,
    pub workspace: &'a dyn WorkspaceClient,
    pub mailbox: &'a Mailbox,
    pub tile: &'a dyn Fn(&str, TilingMode),
    pub open_pane: Option<OpenPane<'a>>,
    pub ledger_worker: Option<&'a str>,
}

impl ExecutionService<'_> {
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
        self.store
            .set_skills(id.as_str(), plan.skills.activated.clone())
            .map_err(SpawnError::Store)?;
        faults.abort_if("abort-after-execution-insert");

        let pane = self
            .launch_stages(&plan, &role)
            .map_err(|(stage, e, pane)| self.launch_failed(&plan, stage, e, pane.as_deref()))?;
        if let Err(e) = self.store.mark_starting(id.as_str(), &pane) {
            eprintln!("horch spawn: recording pane {pane} for {id} failed: {e:#}");
        }
        (self.tile)(&pane, plan.workspace.tiling);
        Ok(SpawnOutcome { plan, pane })
    }

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
        let outcome = self
            .store
            .update(|records| self.claim(records, plan, role, &workspace));
        match outcome {
            Ok(Ok(plan)) => Ok(plan),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(SpawnError::Store(e)),
        }
    }

    fn claim(
        &self,
        records: &mut Vec<LedgerRecordV1>,
        plan: ExecutionPlan,
        role: Option<&str>,
        workspace: &WorkspaceId,
    ) -> Result<Result<ExecutionPlan, PlanError>> {
        let role = self.role_name(role, &plan.launch.teammate.name)?;
        if self.mailbox.role_taken(&role) || planned_role(records, workspace, &role) {
            return Ok(Err(PlanError::RoleTaken(role)));
        }
        let mut plan = finish_plan(plan, RoleName::new(role)?, workspace);
        plan.execution.backend = self.ctx.herdr.backend;
        upsert(records, from_execution(&plan.execution), plan.resumed)?;
        Ok(Ok(plan))
    }

    fn role_name(&self, role: Option<&str>, teammate: &str) -> Result<String> {
        match role {
            Some(role) => Ok(role.to_string()),
            None => Ok(format!("{teammate}-{}", self.mailbox.next_seq(teammate)?)),
        }
    }

    fn write_brief(&self, plan: &ExecutionPlan) -> Result<()> {
        let ctx = self.ctx;
        let text = |p: Option<&Path>| {
            p.map(|p| p.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
        };
        let launch = &plan.launch;
        let e = &plan.execution;
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
            controller: plan.controller.clone(),
            backend: ctx.herdr.backend,
        };
        brief.set_overrides(ctx.bins.overrides.clone());
        brief.worker_id = self.ledger_worker.map(str::to_string);
        brief.ledger_db = text(ctx.paths.ledger_override.as_deref());
        self.mailbox.write_brief(&brief)
    }

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
        let data_root = self.ctx.paths.data_root.to_string_lossy();
        let state_root = self.ctx.paths.state_root.to_string_lossy();
        let mut env = vec![
            ("HORCH_DATA_DIR", data_root.as_ref()),
            ("HORCH_STATE_DIR", state_root.as_ref()),
        ];
        if self.ctx.herdr.backend == crate::runtime::Backend::Hc {
            env.push(("HORCH_BACKEND", "hc"));
        }
        let command = PaneShell::host().command_line_with_env(&exe, &env, &["worker", role]);
        self.workspace.pane_run(pane, &command)
    }

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

fn planned_role(records: &[LedgerRecordV1], workspace: &WorkspaceId, role: &str) -> bool {
    records.iter().any(|r| {
        r.role == role
            && r.workspace_id.as_deref() == Some(workspace.as_str())
            && r.state == Some(ExecutionStatus::Planned)
    })
}

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
