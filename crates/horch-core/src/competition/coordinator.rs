//! The competition round, end to end (B3, CMP-04, CMP-05, CMP-07, CMP-09,
//! CMP-10, CMP-11, CMP-13, CMP-14, ARC-24).
//!
//! A single-threaded loop over the event log. Every step reads the round as
//! `measure::projection::fold` gives it, does the next thing, and records it
//! as an event with an idempotency key, so a coordinator that was killed
//! re-enters the same loop (`resume`) and repeats nothing:
//!
//! | Step | Event | Key |
//! |---|---|---|
//! | plan | `round.created`, `candidate.planned` | `round.created:<round>`, `plan:<round>:<label>` |
//! | worktrees from one base | `worktree.created` | `worktree:<round>:<label>` |
//! | spawn (waves of `safe_n`) | `candidate.spawned` | `spawn:<round>:<label>` |
//! | observe | `candidate.completed` / `candidate.failed` | `ended:<round>:<label>` |
//! | freeze | `candidate.frozen` | `freeze:<round>:<label>` |
//! | validate | `validation.completed` | `validation:<round>:<label>` |
//! | 0 eligible | `winner.rejected{no_eligible}` | `winner.rejected:<round>` |
//!
//! Candidates are ordinary executions (ARC-24): each one is planned with
//! `plan_launch` and started by `ExecutionService` in the dedicated
//! workspace `multi-herdr-dataset <exp8>`, pinned to its planned teammate,
//! working in its own worktree, reporting to nobody. The execution id is
//! minted when the candidate is planned and travels on `candidate.planned`,
//! and the store's key `spawn:<round>:<label>` finds an execution that a
//! killed coordinator already started, so a round never has more than N.
//!
//! With at least 1 eligible candidate the round goes to JUDGING_BACKGROUND
//! and the judge step runs (`judging::start`, then `judging::poll` each
//! tick) until the round is decided; then cleanup removes the worktrees.
//!
//! Fault points (each fires after its event, as [`FaultFired`]):
//! `abort-after-worktree:<n>`, `abort-after-candidate-spawned:<n>`,
//! `abort-after-freeze:<label>`, `abort-after-validation:<label>`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::competition::budget::{BudgetAction, BudgetPolicy, UsageMeter, UsageSource};
use crate::competition::cleanup::{CleanupOptions, RoundCleanup};
use crate::competition::config::DatasetConfig;
use crate::competition::judging::{self, JobLauncher, JudgeEnv, JudgingStatus};
use crate::competition::model::RoundState;
use crate::competition::observe::{
    self, classify, deadline_passed, Observed, TelemetryUsage, CANCELLED_BUDGET, CANCELLED_DISK,
};
use crate::competition::planner::{candidate_planned_payload, round_created_payload, RoundPlan};
use crate::competition::promotion::FaultFired;
use crate::evaluation::scheduler::DEFAULT_STALE_AFTER;
use crate::evaluation::validator::{ValidationReport, Validator};
use crate::evaluation::winner::RejectReason;
use crate::execution::plan::{plan_launch, MintedIds, PlanInputs};
use crate::execution::service::{ExecutionService, SpawnError};
use crate::execution::store::{to_execution, ExecutionStore};
use crate::execution::{
    Execution, ExecutionKind, ExecutionStatus, FailureKind, LaunchStage, ReportTarget,
    SpawnRequest, TilingMode,
};
use crate::fsx::{self, FsxError};
use crate::ids::{ExecutionId, ExperimentId, PaneId, RoundId, SessionId};
use crate::measure::digest::sha256_bytes;
use crate::measure::event::{
    Actor, CandidateCompleted, CandidateFailed, CandidateFrozen, CandidatePlanned,
    CandidateSpawned, EventKind, FinalOutcome, InterventionSource, PromotionIntent,
    RoundNeedsIntervention, ValidationCompleted, WinnerRejected, WorktreeCreated,
};
use crate::measure::paths::DatasetPaths;
use crate::measure::projection::{fold, CandidateView, RoundView};
use crate::measure::recorder::{Appended, JsonlRecorder, NewEvent, Recorder};
use crate::messaging::mailbox::Mailbox;
use crate::prompts::render;
use crate::runtime::fault::Faults;
use crate::runtime::RuntimeContext;
use crate::teammates::Roster;
use crate::usage::money::MicroUsd;
use crate::vcs::git::GitClient;
use crate::vcs::worktree::{FrozenCandidate, WorktreeManager, WorktreeSpec};
use crate::workspace::client::WorkspaceClient;

/// The `_base/` file whose body is every candidate's task text (CMP-15).
pub const CANDIDATE_TEMPLATE: &str = "competition-candidate";
/// The label of a round's herdr workspace: `multi-herdr-dataset <exp8>`.
pub const WORKSPACE_LABEL: &str = "multi-herdr-dataset";

/// The hidden teammate that judges a round.
pub const JUDGE_TEAMMATE: &str = "judge";

pub const ABORT_AFTER_WORKTREE: &str = "abort-after-worktree";
pub const ABORT_AFTER_CANDIDATE_SPAWNED: &str = "abort-after-candidate-spawned";
pub const ABORT_AFTER_FREEZE: &str = "abort-after-freeze";
pub const ABORT_AFTER_VALIDATION: &str = "abort-after-validation";

/// What the coordinator works with. Every adapter is passed in.
pub struct Coordinator<'a, G: GitClient> {
    /// The context candidates are spawned from. Its `bins.current_exe` must
    /// be `horch`: the service runs `<exe> worker <role>` in each pane.
    pub ctx: &'a RuntimeContext,
    pub recorder: &'a JsonlRecorder,
    pub paths: &'a DatasetPaths,
    pub git: &'a G,
    pub workspace: &'a dyn WorkspaceClient,
    pub store: &'a ExecutionStore,
    pub roster: &'a Roster,
    pub validator: &'a dyn Validator,
    pub usage: &'a TelemetryUsage,
    pub meter: &'a UsageMeter,
    /// Free bytes on the worktree disk now (`runtime::machine::probe`).
    pub disk_free: &'a dyn Fn() -> Option<u64>,
    /// A monotonic clock for `occurred_at` and deadlines.
    pub clock: &'a dyn Fn() -> DateTime<Utc>,
    pub sleep: &'a dyn Fn(Duration),
    pub faults: &'a Faults,
    /// Starts a judge job: `DetachedLauncher(ctx)` in the binary.
    pub launcher: &'a dyn JobLauncher,
}

/// One round's fixed inputs.
#[derive(Debug, Clone)]
pub struct RoundSpec {
    pub experiment: ExperimentId,
    pub round: RoundId,
    pub index: u32,
    pub config: DatasetConfig,
    /// The task every candidate works on.
    pub task: String,
    /// The main repository (the project).
    pub repo: PathBuf,
    pub worktree_root: PathBuf,
    /// Candidates live at once (preflight's `safe_n`).
    pub safe_n: u32,
    pub tick: Duration,
    /// What the dataset workspace's root pane runs (`multi-herdr-dataset watch`).
    pub watch_command: String,
}

/// Where a round ended up when [`Coordinator::drive`] returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundOutcome {
    /// The round is complete with a winner, or decided and handed to promotion.
    Decided,
    /// The round was rejected. `budget`: the budget cancelled or stopped a
    /// candidate.
    Rejected {
        budget: bool,
    },
    NeedsIntervention,
}

/// The dataset workspace of an experiment, kept in
/// `experiments/<exp>/workspace.json` so a resume reuses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetWorkspace {
    pub workspace_id: String,
    pub root_pane: String,
}

/// Why no more candidates launch this round.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stop {
    Budget,
    Disk { free: u64, floor: u64 },
}

impl Stop {
    fn reason(&self) -> String {
        match self {
            Stop::Budget => CANCELLED_BUDGET.to_string(),
            Stop::Disk { free, floor } => {
                format!("{CANCELLED_DISK}: {free} bytes free, below the {floor} byte floor")
            }
        }
    }
}

/// The text a candidate is briefed with: the round's task inside the
/// competition rules. Rendered from `_base/competition-candidate.md`; the
/// base prompts are untouched (CMP-15).
pub fn candidate_task(roster: &Roster, task: &str, worktree: &std::path::Path) -> Result<String> {
    let base = roster.require_base(CANDIDATE_TEMPLATE)?;
    let worktree = worktree.to_string_lossy();
    render(
        base.body.trim(),
        &BTreeMap::from([("task", task), ("worktree", worktree.as_ref())]),
    )
}

/// The role a candidate registers as in the dataset workspace.
pub fn candidate_role(label: &str) -> String {
    format!("candidate-{label}")
}

impl<G: GitClient> Coordinator<'_, G> {
    /// Record the planned round, then run it.
    pub fn start(&self, spec: &RoundSpec, plan: &RoundPlan) -> Result<RoundOutcome> {
        self.record_plan(spec, plan)?;
        self.drive(spec)
    }

    /// Record `round.created` and each missing `candidate.planned`, with a
    /// fresh execution id per candidate. Idempotent.
    pub fn record_plan(&self, spec: &RoundSpec, plan: &RoundPlan) -> Result<()> {
        if plan.candidates.is_empty() {
            bail!("the round plan has no candidates");
        }
        self.emit(
            spec,
            EventKind::RoundCreated(round_created_payload(plan)),
            format!("round.created:{}", spec.round),
            None,
        )?;
        let view = self.view(spec)?;
        for slot in &plan.candidates {
            let label = slot.label.to_string();
            if view
                .candidates
                .get(&label)
                .is_some_and(|c| c.planned.is_some())
            {
                continue;
            }
            self.emit(
                spec,
                EventKind::CandidatePlanned(candidate_planned_payload(slot)),
                format!("plan:{}:{label}", spec.round),
                Some(ExecutionId::mint((self.clock)())),
            )?;
        }
        Ok(())
    }

    /// Run the round from wherever its events say it is, to JUDGING_BACKGROUND
    /// or a final state. `resume` calls this directly.
    pub fn drive(&self, spec: &RoundSpec) -> Result<RoundOutcome> {
        let view = self.view(spec)?;
        if matches!(view.state, RoundState::Planned) {
            bail!(
                "round {} is still being planned; plan it again with the same round id",
                spec.round
            );
        }
        self.provision(spec)?;
        if !self
            .view(spec)?
            .candidates
            .values()
            .all(|c| c.is_terminal())
        {
            let ws = self.dataset_workspace(spec)?;
            self.run_candidates(spec, &ws)?;
        }
        self.validate_all(spec)?;
        self.decide(spec)
    }

    // ── provisioning ────────────────────────────────────────────────────

    fn worktree_spec(&self, spec: &RoundSpec, label: &str, base_sha: &str) -> WorktreeSpec {
        WorktreeSpec {
            repo: spec.repo.clone(),
            root: spec.worktree_root.clone(),
            exp8: spec.experiment.as_str().chars().take(8).collect(),
            round_index: spec.index,
            label: label.to_string(),
            base_sha: base_sha.to_string(),
        }
    }

    /// One worktree per candidate, all from the round's base SHA (CMP-04).
    fn provision(&self, spec: &RoundSpec) -> Result<()> {
        let view = self.view(spec)?;
        let base = view.created.base_sha.clone();
        if base.is_empty() {
            bail!("round {} has no base SHA", spec.round);
        }
        let mut made = view
            .candidates
            .values()
            .filter(|c| c.worktree.is_some())
            .count();
        for (label, c) in &view.candidates {
            if c.worktree.is_some() {
                continue;
            }
            let wt = self.worktree_spec(spec, label, &base);
            let mgr = WorktreeManager { git: self.git };
            let path = mgr
                .create(&wt)
                .with_context(|| format!("creating the worktree of {label}"))?;
            let recorded = self.emit(
                spec,
                EventKind::WorktreeCreated(WorktreeCreated {
                    label: label.clone(),
                    path,
                    branch: wt.branch(),
                    base_sha: base.clone(),
                }),
                format!("worktree:{}:{label}", spec.round),
                c.execution_id.clone(),
            )?;
            if recorded {
                made += 1;
                self.fault(&format!("{ABORT_AFTER_WORKTREE}:{made}"))?;
            }
        }
        Ok(())
    }

    // ── the dataset workspace ───────────────────────────────────────────

    fn workspace_file(&self, spec: &RoundSpec) -> Result<PathBuf> {
        Ok(self
            .paths
            .experiment_dir(&spec.experiment)?
            .join("workspace.json"))
    }

    /// The experiment's herdr workspace: reused when its root pane still
    /// exists, else created without focus. Its root pane runs `watch`; no
    /// pane is registered as `orchestrator`.
    fn dataset_workspace(&self, spec: &RoundSpec) -> Result<DatasetWorkspace> {
        let file = self.workspace_file(spec)?;
        if let Some(ws) = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice::<DatasetWorkspace>(&b).ok())
        {
            if self.workspace.pane_get(&ws.root_pane).is_ok() {
                return Ok(ws);
            }
        }
        let exp8: String = spec.experiment.as_str().chars().take(8).collect();
        let repo = spec.repo.to_string_lossy();
        let created = self
            .workspace
            .workspace_create(&format!("{WORKSPACE_LABEL} {exp8}"), Some(&repo), false)
            .context("creating the dataset workspace")?;
        let ws = DatasetWorkspace {
            workspace_id: created.workspace_id,
            root_pane: created.root_pane_id,
        };
        if let Err(e) = self.workspace.pane_run(&ws.root_pane, &spec.watch_command) {
            eprintln!("multi-herdr-dataset: starting watch failed: {e:#}");
        }
        observe::ensure_dirs(self.paths, file.parent().context("workspace file")?)?;
        fsx::write_atomic(&file, &serde_json::to_vec_pretty(&ws)?, 0o600)
            .with_context(|| format!("writing {}", file.display()))?;
        Ok(ws)
    }

    /// Close the dataset workspace once no candidate runs in it. Best effort.
    pub fn close_workspace(&self, spec: &RoundSpec) {
        let Ok(file) = self.workspace_file(spec) else {
            return;
        };
        let Some(ws) = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice::<DatasetWorkspace>(&b).ok())
        else {
            return;
        };
        if self.workspace.pane_get(&ws.root_pane).is_ok() {
            if let Err(e) = self.workspace.workspace_close(&ws.workspace_id) {
                eprintln!("multi-herdr-dataset: closing the dataset workspace failed: {e:#}");
            }
        }
    }

    // ── running ─────────────────────────────────────────────────────────

    /// Launch, observe and end candidates until every one is terminal.
    fn run_candidates(&self, spec: &RoundSpec, ws: &DatasetWorkspace) -> Result<()> {
        let mut stop: Option<Stop> = None;
        loop {
            let view = self.view(spec)?;
            if view.candidates.values().all(CandidateView::is_terminal) {
                return Ok(());
            }
            let records = self.records()?;
            let now = (self.clock)();

            // Observe every started candidate.
            for (label, c) in &view.candidates {
                if c.is_terminal() {
                    continue;
                }
                if let Some(e) = records.get(&self.key(spec, label)) {
                    self.observe(spec, label, c, e, now)?;
                }
            }

            // The live budget (CMP-10).
            let records = self.records()?;
            let spent = self.spent(spec, &view, &records);
            match BudgetPolicy::check(spent, MicroUsd(0), &spec.config.budget) {
                BudgetAction::Continue => {}
                BudgetAction::StopLaunches => {
                    stop.get_or_insert(Stop::Budget);
                }
                BudgetAction::CancelRunning => {
                    stop.get_or_insert(Stop::Budget);
                    self.cancel_live(spec, &records)?;
                }
            }

            // Disk pressure (CMP-11): below the preflight headroom, nothing
            // new starts. Running candidates go on.
            let floor = spec.config.caps.disk_headroom_bytes;
            if let Some(free) = (self.disk_free)() {
                if free < floor && stop.is_none() {
                    eprintln!(
                        "multi-herdr-dataset: disk pressure ({free} bytes free, floor {floor}); \
                         no new candidates start"
                    );
                    stop = Some(Stop::Disk { free, floor });
                }
            }

            let view = self.view(spec)?;
            let records = self.records()?;
            match &stop {
                Some(why) => self.cancel_unlaunched(spec, &view, &records, why)?,
                None => self.launch_wave(spec, ws, &view, &records)?,
            }

            if self
                .view(spec)?
                .candidates
                .values()
                .all(CandidateView::is_terminal)
            {
                return Ok(());
            }
            (self.sleep)(spec.tick);
        }
    }

    /// Every candidate execution of the store, by idempotency key.
    fn records(&self) -> Result<BTreeMap<String, Execution>> {
        Ok(self
            .store
            .read()?
            .iter()
            .filter_map(|r| to_execution(r).ok())
            .filter_map(|e| e.idempotency_key().map(|k| (k, e)))
            .collect())
    }

    fn key(&self, spec: &RoundSpec, label: &str) -> String {
        format!("spawn:{}:{label}", spec.round)
    }

    /// Look at one started candidate and record how it ended, if it did.
    fn observe(
        &self,
        spec: &RoundSpec,
        label: &str,
        c: &CandidateView,
        e: &Execution,
        now: DateTime<Utc>,
    ) -> Result<()> {
        // A record that a killed coordinator started but did not announce.
        if c.spawned.is_none() {
            match &e.pane {
                Some(pane) if e.status != ExecutionStatus::Planned => {
                    self.announce_spawn(spec, label, e, pane)?;
                }
                _ if e.status == ExecutionStatus::Planned => {
                    // Its spawner died before the pane: it never ran.
                    self.store.set_state(
                        e.id.as_str(),
                        ExecutionStatus::LaunchFailed {
                            stage: LaunchStage::Split,
                            reason: "abandoned: the coordinator stopped before the pane ran".into(),
                        },
                    )?;
                    return self.ended(spec, label, e, Observed::Failed(FailureKind::Crashed));
                }
                _ => {}
            }
        }
        let alive = e.pane.as_ref().map(|p| self.pane_alive(p));
        match classify(e, alive) {
            Observed::Live => {
                let started = crate::clock::parse(&e.created_at).unwrap_or(now);
                if deadline_passed(started, now, spec.config.caps.candidate_deadline_s) {
                    self.end_candidate(spec, label, e, FailureKind::TimedOut)?;
                }
                Ok(())
            }
            Observed::Failed(FailureKind::PaneVanished) => {
                // `horch done` marks the record before it closes the pane:
                // read it again before calling the pane vanished.
                let again = self.records()?.remove(&self.key(spec, label));
                match again {
                    Some(a) if a.status.is_live() => {
                        self.end_candidate(spec, label, &a, FailureKind::PaneVanished)
                    }
                    Some(a) => {
                        let alive = a.pane.as_ref().map(|p| self.pane_alive(p));
                        self.ended(spec, label, &a, classify(&a, alive))
                    }
                    None => Ok(()),
                }
            }
            ended => {
                // The agent is gone; its pane may still hold a shell.
                if let (Some(pane), Some(true)) = (&e.pane, alive) {
                    let _ = self.workspace.pane_close(pane.as_str());
                }
                self.ended(spec, label, e, ended)
            }
        }
    }

    fn pane_alive(&self, pane: &PaneId) -> bool {
        self.workspace.pane_get(pane.as_str()).is_ok()
    }

    fn announce_spawn(
        &self,
        spec: &RoundSpec,
        label: &str,
        e: &Execution,
        pane: &PaneId,
    ) -> Result<()> {
        let routing = e
            .routing
            .clone()
            .with_context(|| format!("execution {} has no routing", e.id))?;
        let recorded = self.emit(
            spec,
            EventKind::CandidateSpawned(CandidateSpawned {
                label: label.to_string(),
                pane: pane.clone(),
                routing,
            }),
            self.key(spec, label),
            Some(e.id.clone()),
        )?;
        if recorded {
            let n = self
                .view(spec)?
                .candidates
                .values()
                .filter(|c| c.spawned.is_some())
                .count();
            self.fault(&format!("{ABORT_AFTER_CANDIDATE_SPAWNED}:{n}"))?;
        }
        Ok(())
    }

    /// The coordinator ends a live candidate: the record first, then the
    /// pane (which kills the agent), then the event. A crash between them
    /// leaves a terminal record, which the next look records.
    fn end_candidate(
        &self,
        spec: &RoundSpec,
        label: &str,
        e: &Execution,
        failure: FailureKind,
    ) -> Result<()> {
        self.store.set_state(
            e.id.as_str(),
            ExecutionStatus::Failed {
                failure: failure.clone(),
            },
        )?;
        if let Some(pane) = &e.pane {
            let _ = self.workspace.pane_close(pane.as_str());
        }
        self.ended(spec, label, e, Observed::Failed(failure))
    }

    /// Record a candidate's end. One key for both kinds: a candidate ends
    /// once.
    fn ended(&self, spec: &RoundSpec, label: &str, e: &Execution, how: Observed) -> Result<()> {
        let kind = match how {
            Observed::Live => return Ok(()),
            Observed::Completed { exit_code } => {
                EventKind::CandidateCompleted(CandidateCompleted {
                    label: label.to_string(),
                    exit_code,
                })
            }
            Observed::Failed(failure) => EventKind::CandidateFailed(CandidateFailed {
                label: label.to_string(),
                failure,
            }),
        };
        self.emit(
            spec,
            kind,
            format!("ended:{}:{label}", spec.round),
            Some(e.id.clone()),
        )?;
        Ok(())
    }

    /// What the round's candidates have spent so far.
    fn spent(
        &self,
        spec: &RoundSpec,
        view: &RoundView,
        records: &BTreeMap<String, Execution>,
    ) -> MicroUsd {
        let mut total = 0i64;
        for label in view.candidates.keys() {
            if let Some(e) = records.get(&self.key(spec, label)) {
                let session = observe::session_of(e);
                if let Some(u) = self.usage.usage(e.harness.as_str(), session.as_deref()) {
                    total = total.saturating_add(self.meter.price(&u).0 .0);
                }
            }
        }
        MicroUsd(total)
    }

    /// Cancel every live candidate; its worktree and data stay.
    fn cancel_live(&self, spec: &RoundSpec, records: &BTreeMap<String, Execution>) -> Result<()> {
        let view = self.view(spec)?;
        for (label, c) in &view.candidates {
            if c.is_terminal() {
                continue;
            }
            if let Some(e) = records.get(&self.key(spec, label)) {
                if e.status.is_live() {
                    self.end_candidate(
                        spec,
                        label,
                        e,
                        FailureKind::Cancelled {
                            reason: CANCELLED_BUDGET.into(),
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    /// End every candidate that has not started, with the reason.
    fn cancel_unlaunched(
        &self,
        spec: &RoundSpec,
        view: &RoundView,
        records: &BTreeMap<String, Execution>,
        why: &Stop,
    ) -> Result<()> {
        for (label, c) in &view.candidates {
            if c.is_terminal()
                || c.spawned.is_some()
                || records.contains_key(&self.key(spec, label))
            {
                continue;
            }
            self.emit(
                spec,
                EventKind::CandidateFailed(CandidateFailed {
                    label: label.clone(),
                    failure: FailureKind::Cancelled {
                        reason: why.reason(),
                    },
                }),
                format!("ended:{}:{label}", spec.round),
                c.execution_id.clone(),
            )?;
        }
        Ok(())
    }

    /// Start candidates until `safe_n` are live.
    fn launch_wave(
        &self,
        spec: &RoundSpec,
        ws: &DatasetWorkspace,
        view: &RoundView,
        records: &BTreeMap<String, Execution>,
    ) -> Result<()> {
        let mut live = view
            .candidates
            .iter()
            .filter(|(l, c)| !c.is_terminal() && records.contains_key(&self.key(spec, l)))
            .count() as u32;
        for (label, c) in &view.candidates {
            if live >= spec.safe_n.max(1) {
                break;
            }
            if c.is_terminal()
                || c.spawned.is_some()
                || records.contains_key(&self.key(spec, label))
            {
                continue;
            }
            self.launch(spec, ws, label, c)?;
            live += 1;
        }
        Ok(())
    }

    /// Start one candidate through the execution service (CMP-05, ARC-24).
    fn launch(
        &self,
        spec: &RoundSpec,
        ws: &DatasetWorkspace,
        label: &str,
        c: &CandidateView,
    ) -> Result<()> {
        let planned: &CandidatePlanned = c.planned.as_ref().context("candidate not planned")?;
        let worktree = c.worktree.as_ref().context("candidate has no worktree")?;
        let execution = c
            .execution_id
            .clone()
            .context("candidate.planned carries no execution id")?;
        let task = candidate_task(self.roster, &spec.task, &worktree.path)?;
        let mut req = SpawnRequest::worker(Some(planned.teammate.clone()), task);
        req.kind = ExecutionKind::Candidate {
            experiment: spec.experiment.clone(),
            round: spec.round.clone(),
            label: label.to_string(),
        };
        req.workdir = Some(worktree.path.clone());
        req.report_to = ReportTarget::None;
        req.pinned = true;
        req.effort = planned.effort.clone();
        req.from_pane = Some(ws.root_pane.clone());
        req.tiling = TilingMode::Disabled;
        let catalog = self.roster.skill_catalog()?;
        let now = (self.clock)();
        let ids = MintedIds {
            execution: execution.clone(),
            session: SessionId::new(crate::mint_uuid())?,
        };
        let failed = |reason: String| {
            EventKind::CandidateFailed(CandidateFailed {
                label: label.to_string(),
                failure: FailureKind::Cancelled { reason },
            })
        };
        let plan = match plan_launch(
            &req,
            &PlanInputs {
                roster: self.roster,
                catalog: &catalog,
                gate: None,
                existing: None,
                now,
                ids: &ids,
                project: &spec.repo,
            },
        ) {
            Ok(plan) => plan,
            Err(e) => {
                // Nothing was written: the candidate never runs.
                self.emit(
                    spec,
                    failed(format!("spawn refused: {e}")),
                    format!("ended:{}:{label}", spec.round),
                    Some(execution),
                )?;
                return Ok(());
            }
        };
        let mailbox = Mailbox::in_context(self.ctx, &ws.workspace_id);
        let service = ExecutionService {
            ctx: self.ctx,
            store: self.store,
            workspace: self.workspace,
            mailbox: &mailbox,
            tile: &|_, _| {},
        };
        match service.spawn(plan, Some(&candidate_role(label))) {
            Ok(out) => {
                let e = self
                    .records()?
                    .remove(&self.key(spec, label))
                    .context("the spawned candidate has no record")?;
                let pane = PaneId::new(out.pane)?;
                self.announce_spawn(spec, label, &e, &pane)
            }
            Err(SpawnError::LaunchFailed { stage, source, .. }) => {
                eprintln!("multi-herdr-dataset: candidate {label} failed to launch at {stage:?}: {source:#}");
                self.emit(
                    spec,
                    EventKind::CandidateFailed(CandidateFailed {
                        label: label.to_string(),
                        failure: FailureKind::Crashed,
                    }),
                    format!("ended:{}:{label}", spec.round),
                    Some(execution),
                )?;
                Ok(())
            }
            Err(e @ (SpawnError::Refused { .. } | SpawnError::Plan(_))) => {
                self.emit(
                    spec,
                    failed(format!("spawn refused: {e}")),
                    format!("ended:{}:{label}", spec.round),
                    Some(execution),
                )?;
                Ok(())
            }
            Err(SpawnError::Store(e)) => Err(e),
        }
    }

    // ── freeze and validation ───────────────────────────────────────────

    /// Freeze and validate every terminal candidate (CMP-08, CMP-09). A
    /// candidate that never ran gets a frozen base and an empty, ineligible
    /// report. Eligible means: the candidate completed and every gate passed;
    /// a failed candidate is kept in the round, frozen and validated, but is
    /// never eligible. With 0 gates a completed candidate is eligible with
    /// `mechanical_score` 0.0.
    fn validate_all(&self, spec: &RoundSpec) -> Result<()> {
        let view = self.view(spec)?;
        if view.state != RoundState::Validating {
            return Ok(());
        }
        let records = self.records()?;
        for label in view.candidates.keys() {
            let c = &self.view(spec)?.candidates[label];
            let wt = c.worktree.as_ref().context("candidate has no worktree")?;
            let execution = c
                .execution_id
                .clone()
                .context("candidate.planned carries no execution id")?;
            let record = records.get(&self.key(spec, label));
            let ran = record.is_some_and(|e| {
                !matches!(
                    e.status,
                    ExecutionStatus::LaunchFailed { .. } | ExecutionStatus::Planned
                )
            });
            if c.frozen.is_none() {
                let (head_sha, numstat, diff_digest) = if ran {
                    let f = WorktreeManager { git: self.git }.freeze(
                        &self.worktree_spec(spec, label, &wt.base_sha),
                        &execution,
                        (self.clock)(),
                    )?;
                    (f.head_sha, f.numstat, f.diff_digest)
                } else {
                    (wt.base_sha.clone(), Vec::new(), sha256_bytes(b""))
                };
                if let Some(e) = record {
                    let usage = observe::usage_record(
                        self.usage,
                        self.meter,
                        self.paths,
                        &spec.experiment,
                        &spec.round,
                        e,
                        label,
                        spec.config.retain_transcripts,
                    )?;
                    observe::write_usage_record(self.paths, &spec.experiment, &spec.round, &usage)?;
                }
                let recorded = self.emit(
                    spec,
                    EventKind::CandidateFrozen(CandidateFrozen {
                        label: label.clone(),
                        head_sha,
                        numstat,
                        diff_digest,
                    }),
                    format!("freeze:{}:{label}", spec.round),
                    Some(execution.clone()),
                )?;
                if recorded {
                    self.fault(&format!("{ABORT_AFTER_FREEZE}:{label}"))?;
                }
            }
            let c = &self.view(spec)?.candidates[label];
            if c.validation.is_some() {
                continue;
            }
            let frozen = c.frozen.as_ref().context("candidate not frozen")?;
            let candidate = FrozenCandidate {
                label: label.clone(),
                execution_id: execution.clone(),
                worktree: wt.path.clone(),
                branch: wt.branch.clone(),
                base_sha: wt.base_sha.clone(),
                head_sha: frozen.head_sha.clone(),
                numstat: frozen.numstat.clone(),
                diff_digest: frozen.diff_digest.clone(),
                frozen_at: crate::clock::stamp((self.clock)()),
            };
            let mut report = if ran {
                self.validator.validate(&candidate)?
            } else {
                ValidationReport {
                    validation_id: crate::ids::mint_v7((self.clock)()).to_string(),
                    label: label.clone(),
                    head_sha: candidate.head_sha.clone(),
                    gates: Vec::new(),
                    mechanical_score: 0.0,
                    eligible: false,
                }
            };
            report.eligible = report.eligible && c.completed.is_some();
            let recorded = self.emit(
                spec,
                EventKind::ValidationCompleted(ValidationCompleted {
                    label: label.clone(),
                    report,
                }),
                format!("validation:{}:{label}", spec.round),
                Some(execution),
            )?;
            if recorded {
                self.fault(&format!("{ABORT_AFTER_VALIDATION}:{label}"))?;
            }
        }
        Ok(())
    }

    // ── the decision ────────────────────────────────────────────────────

    fn decide(&self, spec: &RoundSpec) -> Result<RoundOutcome> {
        loop {
            let view = self.view(spec)?;
            match view.state {
                RoundState::Validating => {
                    if !view.candidates.values().all(|c| c.validation.is_some()) {
                        bail!(
                            "round {} is validating with unvalidated candidates",
                            spec.round
                        );
                    }
                    // 0 eligible: the judge is skipped (CMP-14).
                    self.emit(
                        spec,
                        EventKind::WinnerRejected(WinnerRejected {
                            reason: RejectReason::NoEligible,
                        }),
                        format!("winner.rejected:{}", spec.round),
                        None,
                    )?;
                }
                RoundState::JudgingBackground => return self.judge(spec),
                RoundState::Rejected | RoundState::Cleanup => {
                    self.cleanup(spec, &view)?;
                }
                RoundState::Decided => {
                    if view.promotion.started.is_some()
                        || matches!(
                            view.winner.as_ref().map(|w| &w.promotion),
                            Some(crate::measure::event::PromotionIntent::Requested { .. })
                        )
                    {
                        return Ok(RoundOutcome::Decided);
                    }
                    self.cleanup(spec, &view)?;
                }
                RoundState::Complete => {
                    return Ok(match view.final_outcome {
                        Some(FinalOutcome::Rejected) => RoundOutcome::Rejected {
                            budget: budget_stopped(&view),
                        },
                        Some(FinalOutcome::NeedsIntervention) => RoundOutcome::NeedsIntervention,
                        _ => RoundOutcome::Decided,
                    })
                }
                RoundState::NeedsIntervention => return Ok(RoundOutcome::NeedsIntervention),
                RoundState::Revalidating | RoundState::Promoting | RoundState::Promoted => {
                    return Ok(RoundOutcome::Decided)
                }
                other => bail!("round {} cannot be decided in state {other}", spec.round),
            }
        }
    }

    fn cleanup(&self, spec: &RoundSpec, view: &RoundView) -> Result<()> {
        RoundCleanup {
            git: self.git,
            recorder: self.recorder,
            paths: self.paths,
            faults: self.faults,
            repo: spec.repo.clone(),
        }
        .run(
            &spec.round,
            view,
            CleanupOptions {
                prune_branches: spec.config.prune_branches,
                force: false,
            },
        )?;
        Ok(())
    }

    /// JUDGING_BACKGROUND: start the judge job, then poll it each tick
    /// until the round is decided (B4, `competition::judging`). `poll` gets
    /// wall-clock time: it compares heartbeats and file times. A judge bundle
    /// that conflicts with one already on disk is never retried: the round
    /// stops in NEEDS_INTERVENTION.
    fn judge(&self, spec: &RoundSpec) -> Result<RoundOutcome> {
        let config = &spec.config;
        let mut teammate = self.roster.require(JUDGE_TEAMMATE)?.clone();
        teammate.model = Some(config.judge.model.clone());
        teammate.effort = Some(config.judge.effort.clone()).filter(|e| !e.is_empty());
        let env = JudgeEnv {
            ctx: self.ctx,
            recorder: self.recorder,
            paths: self.paths,
            git: self.git,
            repo: &spec.repo,
            store: self.store,
            judge: &teammate,
            task_text: &spec.task,
            policy: &config.judge.policy,
            promotion: match &config.promote_to {
                Some(target) => PromotionIntent::Requested {
                    target: target.clone(),
                },
                None => PromotionIntent::NotRequested,
            },
            timeout: Duration::from_secs(config.judge.timeout_s),
            stale_after: DEFAULT_STALE_AFTER,
            launcher: self.launcher,
            clock: self.clock,
        };
        if let Err(e) = judging::start(&env, &spec.round) {
            return self.judge_error(spec, e);
        }
        loop {
            match judging::poll(&env, &spec.round, Utc::now()) {
                Ok(JudgingStatus::Waiting) => (self.sleep)(spec.tick),
                Ok(JudgingStatus::Decided(_) | JudgingStatus::NeedsIntervention) => {
                    return self.decide(spec)
                }
                Err(e) => return self.judge_error(spec, e),
            }
        }
    }

    /// A bundle conflict stops the round for the operator; any other error
    /// stops the coordinator, and `resume` polls again.
    fn judge_error(&self, spec: &RoundSpec, e: anyhow::Error) -> Result<RoundOutcome> {
        let conflict = e.chain().any(|c| {
            matches!(
                c.downcast_ref::<FsxError>(),
                Some(FsxError::Conflict { .. })
            )
        });
        if !conflict {
            return Err(e);
        }
        self.emit(
            spec,
            EventKind::RoundNeedsIntervention(RoundNeedsIntervention {
                reason: format!("judge bundle conflict: {e:#}"),
                source: InterventionSource::Operator,
            }),
            format!("needs_intervention:{}", spec.round),
            None,
        )?;
        Ok(RoundOutcome::NeedsIntervention)
    }

    // ── events ──────────────────────────────────────────────────────────

    /// The round as the events give it.
    pub fn view(&self, spec: &RoundSpec) -> Result<RoundView> {
        let events = self.recorder.read_all()?.events;
        fold(&events)
            .rounds
            .remove(&spec.round)
            .with_context(|| format!("round {} has no events", spec.round))
    }

    /// Append one coordinator event. True when it was recorded now; false
    /// when an earlier run recorded it.
    fn emit(
        &self,
        spec: &RoundSpec,
        kind: EventKind,
        key: String,
        execution_id: Option<ExecutionId>,
    ) -> Result<bool> {
        let appended = self.recorder.append(NewEvent {
            kind,
            actor: Actor::Coordinator,
            experiment_id: spec.experiment.clone(),
            round_id: Some(spec.round.clone()),
            execution_id,
            idempotency_key: key,
            occurred_at: (self.clock)(),
        })?;
        Ok(matches!(appended, Appended::Recorded(_)))
    }

    fn fault(&self, point: &str) -> Result<()> {
        if self.faults.has(point) {
            return Err(FaultFired(point.to_string()).into());
        }
        Ok(())
    }
}

/// Whether the budget ended a candidate of this round.
fn budget_stopped(view: &RoundView) -> bool {
    view.candidates.values().any(|c| {
        matches!(
            c.failed.as_ref().map(|f| &f.failure),
            Some(FailureKind::Cancelled { reason }) if reason == CANCELLED_BUDGET
        )
    })
}

/// An `occurred_at` clock that never goes backwards and never repeats a
/// millisecond, so the event log's day files stay in order (b1-measure).
pub fn monotonic(now: impl Fn() -> DateTime<Utc>) -> impl Fn() -> DateTime<Utc> {
    let last = std::cell::Cell::new(None::<DateTime<Utc>>);
    move || {
        let mut t = now();
        if let Some(prev) = last.get() {
            if t <= prev {
                t = prev + chrono::Duration::milliseconds(1);
            }
        }
        last.set(Some(t));
        t
    }
}
