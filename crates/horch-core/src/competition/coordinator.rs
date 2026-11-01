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
//! tick) until the round is decided. Without a requested promotion, cleanup
//! then removes the worktrees (OD5: collect only). With `--promote-to` or an
//! operator `promote`, the round goes through `RoundPromoter` (REVALIDATING
//! → PROMOTING → PROMOTED) and is cleaned up once the receipt is on disk.
//!
//! Fault points (each fires after its event, as [`FaultFired`]):
//! `abort-after-worktree:<n>`, `abort-after-candidate-spawned:<n>`,
//! `abort-after-freeze:<label>`, `abort-after-validation:<label>`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::competition::budget::{BudgetAction, BudgetPolicy, UsageMeter, UsageSource};
use crate::competition::cleanup::{CleanupOptions, CleanupOutcome, RoundCleanup};
use crate::competition::config::DatasetConfig;
use crate::competition::judging::{self, JobLauncher, JudgeEnv, JudgingStatus};
use crate::competition::model::RoundState;
use crate::competition::observe::{
    self, classify, deadline_passed, trust_dialog, IdleAction, IdleWatch, Observed, TelemetryUsage,
    CANCELLED_BUDGET, CANCELLED_DISK, CANCELLED_IDLE, CANCELLED_TRUST, IDLE_NUDGE,
    TRUST_DIALOG_WINDOW_S,
};
use crate::competition::planner::{candidate_planned_payload, round_created_payload, RoundPlan};
use crate::competition::promotion::{
    BranchRef, FaultFired, GitPromotionEngine, PromotionEngine, PromotionPlan, PromotionResult,
};
use crate::evaluation::validator::{ValidationReport, Validator};
use crate::evaluation::winner::RejectReason;
use crate::execution::plan::{plan_launch, MintedIds, PlanError, PlanInputs};
use crate::execution::service::{ExecutionService, SpawnError, SpawnOutcome};
use crate::execution::store::{to_execution, ExecutionStore};
use crate::execution::{
    Execution, ExecutionKind, ExecutionPlan, ExecutionStatus, FailureKind, LaunchStage,
    ReportTarget, SpawnRequest, TilingMode,
};
use crate::fsx::{self, FsxError};
use crate::heartbeat;
use crate::ids::{ExecutionId, ExperimentId, PaneId, RoundId, SessionId};
use crate::measure::digest::sha256_bytes;
use crate::measure::event::{
    Actor, CandidateCompleted, CandidateFailed, CandidateFrozen, CandidatePlanned,
    CandidateSpawned, EventEnvelope, EventKind, FinalOutcome, InterventionSource, PromotionIntent,
    RoundNeedsIntervention, ValidationCompleted, WinnerRejected, WorktreeCreated,
};
use crate::measure::paths::DatasetPaths;
use crate::measure::projection::{fold, CandidateView, RoundView};
use crate::measure::recorder::{Appended, JsonlRecorder, NewEvent, Recorder};
use crate::messaging::delivery::send_line;
use crate::messaging::mailbox::Mailbox;
use crate::prompts::render;
use crate::roster::{Roster, Teammate};
use crate::runtime::fault::Faults;
use crate::runtime::RuntimeContext;
use crate::skills::{harness_support, SkillResolution};
use crate::usage::money::MicroUsd;
use crate::vcs::git::{GitClient, GitIdentity};
use crate::vcs::worktree::{
    FrozenCandidate, WorktreeManager, WorktreeSpec, FREEZE_EMAIL, FREEZE_NAME,
};
use crate::workspace::client::WorkspaceClient;

/// The `_base/` file whose body is every candidate's task text (CMP-15).
pub(crate) const CANDIDATE_TEMPLATE: &str = "competition-candidate";

/// The label of a round's herdr workspace: `multi-herdr-dataset <exp8>`.
pub const WORKSPACE_LABEL: &str = "multi-herdr-dataset";

/// How often the budget meter and the disk probe run.
pub(crate) const CHECK_EVERY: Duration = Duration::from_secs(5);

/// The hidden teammate that judges a round.
pub(crate) const JUDGE_TEAMMATE: &str = "judge";

pub(crate) const ABORT_AFTER_WORKTREE: &str = "abort-after-worktree";
pub(crate) const ABORT_AFTER_CANDIDATE_SPAWNED: &str = "abort-after-candidate-spawned";
pub(crate) const ABORT_AFTER_FREEZE: &str = "abort-after-freeze";
pub(crate) const ABORT_AFTER_VALIDATION: &str = "abort-after-validation";

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

/// What the run loop carries from one tick to the next.
struct RunState {
    /// Why no more candidates launch, once that is so.
    stop: Option<Stop>,
    /// What each started candidate has spent, by label, as of the last
    /// check.
    measured: BTreeMap<String, i64>,
    tick: u128,
    /// Ticks between two checks. The meter reads whole transcripts and the
    /// probe starts processes: both run on the first tick and then once per
    /// [`CHECK_EVERY`].
    every: u128,
    /// How long each candidate has waited at its prompt (4.11.5).
    idle: BTreeMap<String, IdleWatch>,
}

impl RunState {
    fn new(spec: &RoundSpec) -> Self {
        RunState {
            stop: None,
            measured: BTreeMap::new(),
            tick: 0,
            every: (CHECK_EVERY.as_millis() / spec.tick.as_millis().max(1)).max(1),
            idle: BTreeMap::new(),
        }
    }

    /// Whether this tick is a check tick. Counts the tick.
    fn next_check(&mut self) -> bool {
        let check = self.tick.is_multiple_of(self.every);
        self.tick += 1;
        check
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

/// Record `round.created` and each missing `candidate.planned` of `plan`,
/// with a fresh execution id per candidate. Idempotent. `run --detach`
/// records the plan this way before its pane resumes the round (FDS-10).
pub fn record_round_plan(
    recorder: &JsonlRecorder,
    experiment: &ExperimentId,
    round: &RoundId,
    plan: &RoundPlan,
    clock: &dyn Fn() -> DateTime<Utc>,
) -> Result<()> {
    if plan.candidates.is_empty() {
        bail!("the round plan has no candidates");
    }
    let emit = |kind: EventKind, key: String, execution_id: Option<ExecutionId>| {
        recorder.append(NewEvent {
            kind,
            actor: Actor::Coordinator,
            experiment_id: experiment.clone(),
            round_id: Some(round.clone()),
            execution_id,
            idempotency_key: key,
            occurred_at: clock(),
        })
    };
    emit(
        EventKind::RoundCreated(round_created_payload(plan)),
        format!("round.created:{round}"),
        None,
    )?;
    let view = fold(&recorder.read_all()?.events)
        .rounds
        .remove(round)
        .with_context(|| format!("round {round} has no events"))?;
    for slot in &plan.candidates {
        let label = slot.label.to_string();
        if view
            .candidates
            .get(&label)
            .is_some_and(|c| c.planned.is_some())
        {
            continue;
        }
        emit(
            EventKind::CandidatePlanned(candidate_planned_payload(slot)),
            format!("plan:{round}:{label}"),
            Some(ExecutionId::mint(clock())),
        )?;
    }
    Ok(())
}

/// The herdr workspace of `experiment`: reused when its root pane still
/// exists, else created without focus as `multi-herdr-dataset <exp8>`. Its
/// root pane runs `watch_command`; no pane is registered as
/// `orchestrator`. Kept in `experiments/<exp>/workspace.json`.
pub fn dataset_workspace(
    workspace: &dyn WorkspaceClient,
    paths: &DatasetPaths,
    experiment: &ExperimentId,
    repo: &Path,
    watch_command: &str,
) -> Result<DatasetWorkspace> {
    let file = paths.experiment_dir(experiment)?.join("workspace.json");
    match live_workspace(workspace, &file) {
        Some(ws) => Ok(ws),
        None => create_workspace(workspace, paths, experiment, repo, watch_command, &file),
    }
}

/// The workspace in `file`, when its root pane still exists.
fn live_workspace(workspace: &dyn WorkspaceClient, file: &Path) -> Option<DatasetWorkspace> {
    std::fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice::<DatasetWorkspace>(&b).ok())
        .filter(|ws| workspace.pane_get(&ws.root_pane).is_ok())
}

fn create_workspace(
    workspace: &dyn WorkspaceClient,
    paths: &DatasetPaths,
    experiment: &ExperimentId,
    repo: &Path,
    watch_command: &str,
    file: &Path,
) -> Result<DatasetWorkspace> {
    let repo = repo.to_string_lossy();
    let created = workspace
        .workspace_create(&workspace_name(experiment), Some(&repo), false)
        .context("creating the dataset workspace")?;
    let ws = DatasetWorkspace {
        workspace_id: created.workspace_id,
        root_pane: created.root_pane_id,
    };
    if let Err(e) = workspace.pane_run(&ws.root_pane, watch_command) {
        eprintln!("multi-herdr-dataset: starting watch failed: {e:#}");
    }
    observe::ensure_dirs(paths, file.parent().context("workspace file")?)?;
    fsx::write_atomic(file, &serde_json::to_vec_pretty(&ws)?, 0o600)
        .with_context(|| format!("writing {}", file.display()))?;
    Ok(ws)
}

/// The name of `experiment`'s herdr workspace: `multi-herdr-dataset <exp8>`.
pub fn workspace_name(experiment: &ExperimentId) -> String {
    format!("{WORKSPACE_LABEL} {}", experiment.short())
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
    pub(crate) fn record_plan(&self, spec: &RoundSpec, plan: &RoundPlan) -> Result<()> {
        record_round_plan(
            self.recorder,
            &spec.experiment,
            &spec.round,
            plan,
            self.clock,
        )
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
        // A round stopped for the operator stays stopped: only `promote` or
        // `cleanup` moves it on.
        if matches!(view.state, RoundState::NeedsIntervention) || !self.provision(spec)? {
            Ok(RoundOutcome::NeedsIntervention)
        } else {
            self.run_round(spec)
        }
    }

    /// Provisioned: run the candidates, validate them and decide.
    fn run_round(&self, spec: &RoundSpec) -> Result<RoundOutcome> {
        if !self.all_terminal(spec)? {
            let ws = self.dataset_workspace(spec)?;
            self.run_candidates(spec, &ws)?;
        }
        self.validate_all(spec)?;
        self.decide(spec)
    }

    fn all_terminal(&self, spec: &RoundSpec) -> Result<bool> {
        Ok(self
            .view(spec)?
            .candidates
            .values()
            .all(CandidateView::is_terminal))
    }

    // ── provisioning ────────────────────────────────────────────────────

    fn worktree_spec(
        &self,
        spec: &RoundSpec,
        view: &RoundView,
        label: &str,
        base_sha: &str,
    ) -> WorktreeSpec {
        WorktreeSpec {
            repo: spec.repo.clone(),
            root: spec.worktree_root.clone(),
            exp8: exp8_of(spec, view),
            round_index: spec.index,
            label: label.to_string(),
            base_sha: base_sha.to_string(),
        }
    }

    /// One worktree per candidate, all from the round's base SHA (CMP-04).
    /// `false` when a worktree cannot be made: the round then needs the
    /// operator, and `cleanup --force` removes the worktrees already made.
    fn provision(&self, spec: &RoundSpec) -> Result<bool> {
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
        let missing = view.candidates.iter().filter(|(_, c)| c.worktree.is_none());
        for (label, c) in missing {
            if !self.provision_one(spec, &view, label, c, &base, &mut made)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The worktree of one candidate. `false` when it cannot be made.
    fn provision_one(
        &self,
        spec: &RoundSpec,
        view: &RoundView,
        label: &str,
        c: &CandidateView,
        base: &str,
        made: &mut usize,
    ) -> Result<bool> {
        let wt = self.worktree_spec(spec, view, label, base);
        match (WorktreeManager { git: self.git }).create(&wt) {
            Err(e) => {
                self.emit_worktree_failed(spec, label, &e)?;
                Ok(false)
            }
            Ok(path) => {
                let recorded = self.emit(
                    spec,
                    EventKind::WorktreeCreated(WorktreeCreated {
                        label: label.to_string(),
                        path,
                        branch: wt.branch(),
                        base_sha: base.to_string(),
                    }),
                    format!("worktree:{}:{label}", spec.round),
                    c.execution_id.clone(),
                )?;
                if recorded {
                    *made += 1;
                    self.fault(&format!("{ABORT_AFTER_WORKTREE}:{made}"))?;
                }
                Ok(true)
            }
        }
    }

    fn emit_worktree_failed(&self, spec: &RoundSpec, label: &str, e: &anyhow::Error) -> Result<()> {
        self.emit(
            spec,
            EventKind::RoundNeedsIntervention(RoundNeedsIntervention {
                reason: format!(
                    "creating the worktree of {label}: {e:#}; \
                     `cleanup {} --force` removes the worktrees already made",
                    spec.round
                ),
                source: InterventionSource::Operator,
            }),
            format!("needs_intervention:{}", spec.round),
            None,
        )
        .map(drop)
    }

    // ── the dataset workspace ───────────────────────────────────────────

    fn workspace_file(&self, spec: &RoundSpec) -> Result<PathBuf> {
        Ok(self
            .paths
            .experiment_dir(&spec.experiment)?
            .join("workspace.json"))
    }

    /// The experiment's herdr workspace ([`dataset_workspace`]).
    fn dataset_workspace(&self, spec: &RoundSpec) -> Result<DatasetWorkspace> {
        dataset_workspace(
            self.workspace,
            self.paths,
            &spec.experiment,
            &spec.repo,
            &spec.watch_command,
        )
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
        let mut st = RunState::new(spec);
        while !self.run_tick(spec, ws, &mut st)? {
            (self.sleep)(spec.tick);
        }
        Ok(())
    }

    /// One tick of the run loop. True when every candidate is terminal.
    fn run_tick(&self, spec: &RoundSpec, ws: &DatasetWorkspace, st: &mut RunState) -> Result<bool> {
        let check = st.next_check();
        if self.all_terminal(spec)? {
            Ok(true)
        } else {
            self.observe_all(spec, st)?;
            let (spent, committed) = self.budget_tick(spec, st, check)?;
            self.disk_tick(spec, st, check)?;
            self.launch_tick(spec, ws, st, spent, committed)?;
            self.all_terminal(spec)
        }
    }

    /// Observe every started candidate.
    fn observe_all(&self, spec: &RoundSpec, st: &mut RunState) -> Result<()> {
        let view = self.view(spec)?;
        let records = self.records()?;
        let now = (self.clock)();
        for (label, c) in view.candidates.iter().filter(|(_, c)| !c.is_terminal()) {
            if let Some(e) = records.get(&self.key(spec, label)) {
                let watch = st.idle.entry(label.clone()).or_default();
                self.observe(spec, label, c, e, now, watch)?;
            }
        }
        Ok(())
    }

    /// The live budget (CMP-10). Measured spend is read on check ticks; the
    /// policy runs every tick, so a launch never misses what the running
    /// candidates are committed to. Committed spend (Spec B §4.11): each
    /// running candidate counts max(measured, projected), and the judge's
    /// reserve is held in the policy's limit until it completes (see
    /// `budget`). Each launch must also fit its own projection
    /// (`launch_wave`). Gives the spend and the committed spend.
    fn budget_tick(
        &self,
        spec: &RoundSpec,
        st: &mut RunState,
        check: bool,
    ) -> Result<(MicroUsd, MicroUsd)> {
        let records = self.records()?;
        let view = self.view(spec)?;
        if check {
            st.measured = self.measured(spec, &view, &records);
        }
        let spent = MicroUsd(st.measured.values().fold(0i64, |a, m| a.saturating_add(*m)));
        let committed = self.committed(spec, &view, &records, &st.measured);
        match BudgetPolicy::check(spent, committed, &spec.config.budget) {
            BudgetAction::Continue => {}
            BudgetAction::StopLaunches => {
                st.stop.get_or_insert(Stop::Budget);
            }
            BudgetAction::CancelRunning => {
                st.stop.get_or_insert(Stop::Budget);
                self.cancel_live(spec, &records)?;
            }
        }
        Ok((spent, committed))
    }

    /// Disk pressure (CMP-11): below the preflight headroom, nothing new
    /// starts. Running candidates go on. Checked before every launch, too.
    fn disk_tick(&self, spec: &RoundSpec, st: &mut RunState, check: bool) -> Result<()> {
        let floor = spec.config.caps.disk_headroom_bytes;
        let view = self.view(spec)?;
        let records = self.records()?;
        let launching = st.stop.is_none() && self.next_launch(spec, &view, &records).is_some();
        let free = (check || launching).then(|| (self.disk_free)()).flatten();
        if let Some(free) = free.filter(|free| *free < floor && st.stop.is_none()) {
            eprintln!(
                "multi-herdr-dataset: disk pressure ({free} bytes free, floor {floor}); \
                 no new candidates start"
            );
            st.stop = Some(Stop::Disk { free, floor });
        }
        Ok(())
    }

    /// Launch the next wave, or end every candidate that has not started
    /// once launches have stopped.
    fn launch_tick(
        &self,
        spec: &RoundSpec,
        ws: &DatasetWorkspace,
        st: &mut RunState,
        spent: MicroUsd,
        committed: MicroUsd,
    ) -> Result<()> {
        if st.stop.is_none() && self.try_launch(spec, ws, spent, committed)? {
            st.stop = Some(Stop::Budget);
        }
        match &st.stop {
            Some(why) => self.cancel_unlaunched(spec, &self.view(spec)?, &self.records()?, why),
            None => Ok(()),
        }
    }

    /// Launch one wave; true when the budget blocks the next launch.
    fn try_launch(
        &self,
        spec: &RoundSpec,
        ws: &DatasetWorkspace,
        spent: MicroUsd,
        committed: MicroUsd,
    ) -> Result<bool> {
        let view = self.view(spec)?;
        let records = self.records()?;
        self.launch_wave(spec, ws, &view, &records, spent, committed)
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
        idle: &mut IdleWatch,
    ) -> Result<()> {
        if c.spawned.is_none() && self.observe_unspawned(spec, label, e)? {
            return Ok(());
        }
        let alive = e.pane.as_ref().map(|p| self.pane_alive(p));
        match classify(e, alive) {
            Observed::Live => self.observe_live(spec, label, e, now, idle),
            Observed::Failed(FailureKind::PaneVanished) => self.recheck_vanished(spec, label),
            ended => {
                // The agent is gone; its pane may still hold a shell.
                if let (Some(pane), Some(true)) = (&e.pane, alive) {
                    let _ = self.workspace.pane_close(pane.as_str());
                }
                self.ended(spec, label, e, ended)
            }
        }
    }

    /// A record that a killed coordinator started but did not announce. True
    /// when the candidate needs no further look.
    fn observe_unspawned(&self, spec: &RoundSpec, label: &str, e: &Execution) -> Result<bool> {
        if e.status == ExecutionStatus::Planned {
            self.abandon_launch(spec, label, e)?;
            return Ok(true);
        }
        if let Some(pane) = &e.pane {
            self.announce_spawn(spec, label, e, pane)?;
        }
        Ok(false)
    }

    /// The spawner died before the pane: the candidate never ran.
    fn abandon_launch(&self, spec: &RoundSpec, label: &str, e: &Execution) -> Result<()> {
        let end = self.store.end_live(
            e.id.as_str(),
            ExecutionStatus::LaunchFailed {
                stage: LaunchStage::Split,
                reason: "abandoned: the coordinator stopped before the pane ran".into(),
            },
        )?;
        if matches!(end, ExecutionStatus::LaunchFailed { .. }) {
            self.ended(spec, label, e, Observed::Failed(FailureKind::Crashed))
        } else {
            // It did run after this look: the next look sees it.
            Ok(())
        }
    }

    /// A live candidate: end it when its pane waits at a trust dialog or at
    /// its prompt too long, or when its deadline has passed.
    fn observe_live(
        &self,
        spec: &RoundSpec,
        label: &str,
        e: &Execution,
        now: DateTime<Utc>,
        idle: &mut IdleWatch,
    ) -> Result<()> {
        let started = crate::clock::parse(&e.created_at).unwrap_or(now);
        let cancelled = |reason: &str| FailureKind::Cancelled {
            reason: reason.to_string(),
        };
        let failure = if self.trust_blocked(e, started, now) {
            Some(cancelled(CANCELLED_TRUST))
        } else if self.idle_step(spec, label, e, now, idle) == IdleAction::End {
            Some(cancelled(CANCELLED_IDLE))
        } else if deadline_passed(started, now, spec.config.caps.candidate_deadline_s) {
            Some(FailureKind::TimedOut)
        } else {
            None
        };
        match failure {
            Some(failure) => self.end_candidate(spec, label, e, failure),
            None => Ok(()),
        }
    }

    /// A pane stopped at a trust dialog never starts: end it now, not at the
    /// deadline (PRE-14 should have refused the round).
    fn trust_blocked(&self, e: &Execution, started: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        e.pane.as_ref().is_some_and(|pane| {
            !deadline_passed(started, now, TRUST_DIALOG_WINDOW_S)
                && self
                    .workspace
                    .pane_read(pane.as_str(), "visible")
                    .is_ok_and(|screen| trust_dialog(&screen).is_some())
        })
    }

    /// An agent that waits at its prompt without `horch done` gets one
    /// nudge, then ends instead of waiting for the deadline (dataset design
    /// 4.11.5). Sends the nudge when it is due.
    fn idle_step(
        &self,
        spec: &RoundSpec,
        label: &str,
        e: &Execution,
        now: DateTime<Utc>,
        idle: &mut IdleWatch,
    ) -> IdleAction {
        let caps = &spec.config.caps;
        let Some(pane) = e.pane.as_ref().filter(|_| caps.idle_nudge_after_s > 0) else {
            return IdleAction::Wait;
        };
        let status = self
            .workspace
            .pane_get(pane.as_str())
            .ok()
            .and_then(|p| p.agent_status);
        let action = idle.look(
            status.as_deref(),
            now,
            caps.idle_nudge_after_s,
            caps.idle_end_after_s,
        );
        if action == IdleAction::Nudge {
            eprintln!(
                "multi-herdr-dataset: candidate {label} waits at its prompt \
                 without horch done; nudging it once"
            );
            if let Err(err) = send_line(self.workspace, pane.as_str(), IDLE_NUDGE) {
                eprintln!("multi-herdr-dataset: nudging candidate {label}: {err:#}");
            }
        }
        action
    }

    /// `horch done` marks the record before it closes the pane: read it
    /// again before calling the pane vanished.
    fn recheck_vanished(&self, spec: &RoundSpec, label: &str) -> Result<()> {
        match self.records()?.remove(&self.key(spec, label)) {
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
        let ours = ExecutionStatus::Failed { failure };
        let end = self.store.end_live(e.id.as_str(), ours.clone())?;
        if let Some(pane) = &e.pane {
            let _ = self.workspace.pane_close(pane.as_str());
        }
        if end == ours {
            let ExecutionStatus::Failed { failure } = end else {
                unreachable!("ours is a failure")
            };
            return self.ended(spec, label, e, Observed::Failed(failure));
        }
        // The record ended before this write (the agent ran `horch done`
        // after the look): record that end, not ours.
        let ended = self
            .records()?
            .remove(&self.key(spec, label))
            .unwrap_or_else(|| Execution {
                status: end,
                ..e.clone()
            });
        self.ended(spec, label, &ended, classify(&ended, None))
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

    /// What each started candidate of the round has spent so far, by label.
    fn measured(
        &self,
        spec: &RoundSpec,
        view: &RoundView,
        records: &BTreeMap<String, Execution>,
    ) -> BTreeMap<String, i64> {
        let mut spent = BTreeMap::new();
        for label in view.candidates.keys() {
            if let Some(e) = records.get(&self.key(spec, label)) {
                let session = observe::session_of(e);
                if let Some(u) = self.usage.usage(e.harness.as_str(), session.as_deref()) {
                    spent.insert(label.clone(), self.meter.price(&u).0 .0);
                }
            }
        }
        spent
    }

    /// The further spend the running candidates are expected to make: for
    /// each, its projected cost less what it has spent, never below 0.
    fn committed(
        &self,
        spec: &RoundSpec,
        view: &RoundView,
        records: &BTreeMap<String, Execution>,
        measured: &BTreeMap<String, i64>,
    ) -> MicroUsd {
        let total = view
            .candidates
            .iter()
            .filter(|(label, c)| self.live(spec, label, c, records))
            .filter_map(|(label, c)| c.planned.as_ref().map(|planned| (label, planned)))
            .fold(0i64, |total, (label, planned)| {
                let projected = self.meter.projected(planned.model.as_str()).0;
                let spent = measured.get(label).copied().unwrap_or(0);
                total.saturating_add(projected.saturating_sub(spent).max(0))
            });
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

    /// A candidate that is not terminal and has a record.
    fn live(
        &self,
        spec: &RoundSpec,
        label: &str,
        c: &CandidateView,
        records: &BTreeMap<String, Execution>,
    ) -> bool {
        !c.is_terminal() && records.contains_key(&self.key(spec, label))
    }

    /// A candidate that has no record yet and was not announced: it has not
    /// started.
    fn unlaunched(
        &self,
        spec: &RoundSpec,
        label: &str,
        c: &CandidateView,
        records: &BTreeMap<String, Execution>,
    ) -> bool {
        !c.is_terminal() && c.spawned.is_none() && !records.contains_key(&self.key(spec, label))
    }

    /// The candidates that are live: not terminal, and with a record.
    fn live_count(
        &self,
        spec: &RoundSpec,
        view: &RoundView,
        records: &BTreeMap<String, Execution>,
    ) -> u32 {
        view.candidates
            .iter()
            .filter(|(l, c)| self.live(spec, l, c, records))
            .count() as u32
    }

    /// End every candidate that has not started, with the reason.
    fn cancel_unlaunched(
        &self,
        spec: &RoundSpec,
        view: &RoundView,
        records: &BTreeMap<String, Execution>,
        why: &Stop,
    ) -> Result<()> {
        let unlaunched = view
            .candidates
            .iter()
            .filter(|(label, c)| self.unlaunched(spec, label, c, records));
        for (label, c) in unlaunched {
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

    /// The candidate that would launch now, if `safe_n` allows one.
    fn next_launch<'v>(
        &self,
        spec: &RoundSpec,
        view: &'v RoundView,
        records: &BTreeMap<String, Execution>,
    ) -> Option<&'v String> {
        if self.live_count(spec, view, records) >= spec.safe_n.max(1) {
            return None;
        }
        view.candidates
            .iter()
            .find(|(l, c)| self.unlaunched(spec, l, c, records))
            .map(|(l, _)| l)
    }

    /// Start candidates until `safe_n` are live, in label order, while
    /// each one's projection fits the budget on top of `spent` and
    /// `committed` (Spec B §4.11). True when a candidate does not fit and
    /// none is live: no end of a running candidate can make room.
    fn launch_wave(
        &self,
        spec: &RoundSpec,
        ws: &DatasetWorkspace,
        view: &RoundView,
        records: &BTreeMap<String, Execution>,
        spent: MicroUsd,
        mut committed: MicroUsd,
    ) -> Result<bool> {
        let live = self.live_count(spec, view, records);
        let room = spec.safe_n.max(1).saturating_sub(live) as usize;
        let launchable = view
            .candidates
            .iter()
            .filter(|(label, c)| self.unlaunched(spec, label, c, records))
            .take(room);
        for (live, (label, c)) in (live..).zip(launchable) {
            let next = c
                .planned
                .as_ref()
                .map_or(MicroUsd(0), |p| self.meter.projected(p.model.as_str()));
            if !BudgetPolicy::fits(spent, committed, next, &spec.config.budget) {
                return Ok(live == 0);
            }
            self.launch(spec, ws, label, c)?;
            committed = MicroUsd(committed.0.saturating_add(next.0));
        }
        Ok(false)
    }

    /// Start one candidate through the execution service (CMP-05, ARC-24).
    fn launch(
        &self,
        spec: &RoundSpec,
        ws: &DatasetWorkspace,
        label: &str,
        c: &CandidateView,
    ) -> Result<()> {
        let (req, execution) = self.candidate_request(spec, ws, label, c)?;
        match self.plan_candidate(spec, &req, &execution)? {
            // Nothing was written: the candidate never runs.
            Err(e) => self.end_unlaunched(
                spec,
                label,
                execution,
                FailureKind::Cancelled {
                    reason: format!("spawn refused: {e}"),
                },
            ),
            Ok(plan) => {
                let mailbox = Mailbox::in_context(self.ctx, &ws.workspace_id);
                let service = ExecutionService {
                    ctx: self.ctx,
                    store: self.store,
                    workspace: self.workspace,
                    mailbox: &mailbox,
                    tile: &|_, _| {},
                    ledger_worker: None,
                };
                let outcome = service.spawn(plan, Some(&candidate_role(label)));
                self.record_spawn(spec, label, execution, outcome)
            }
        }
    }

    /// The request that starts the candidate, and its execution id.
    fn candidate_request(
        &self,
        spec: &RoundSpec,
        ws: &DatasetWorkspace,
        label: &str,
        c: &CandidateView,
    ) -> Result<(SpawnRequest, ExecutionId)> {
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
        Ok((req, execution))
    }

    /// Plan the launch of `req`. The inner error is a plan the launch
    /// planner refuses.
    fn plan_candidate(
        &self,
        spec: &RoundSpec,
        req: &SpawnRequest,
        execution: &ExecutionId,
    ) -> Result<std::result::Result<ExecutionPlan, PlanError>> {
        // As in `horch spawn`: planning reads no files, so the candidate's
        // operator and plugin skills are read here, and its record lists them.
        let skills = SkillResolution::read(
            self.roster.skill_catalog()?,
            req.teammate
                .as_ref()
                .and_then(|t| self.roster.get(t.as_str())),
            self.ctx.inherited.home_var.as_deref().map(Path::new),
            harness_support,
        )?;
        let ids = MintedIds {
            execution: execution.clone(),
            session: SessionId::new(crate::mint_uuid())?,
        };
        Ok(plan_launch(
            req,
            &PlanInputs {
                roster: self.roster,
                skills: &skills,
                gate: None,
                existing: None,
                now: (self.clock)(),
                ids: &ids,
                project: &spec.repo,
            },
        ))
    }

    /// Record how the service's spawn of a candidate went: 1 arm per kind
    /// of failure.
    fn record_spawn(
        &self,
        spec: &RoundSpec,
        label: &str,
        execution: ExecutionId,
        outcome: std::result::Result<SpawnOutcome, SpawnError>,
    ) -> Result<()> {
        match outcome {
            Ok(out) => {
                let e = self
                    .records()?
                    .remove(&self.key(spec, label))
                    .context("the spawned candidate has no record")?;
                self.announce_spawn(spec, label, &e, &PaneId::new(out.pane)?)
            }
            Err(SpawnError::LaunchFailed { stage, source, .. }) => {
                eprintln!("multi-herdr-dataset: candidate {label} failed to launch at {stage:?}: {source:#}");
                self.end_unlaunched(spec, label, execution, FailureKind::Crashed)
            }
            Err(e @ (SpawnError::Refused { .. } | SpawnError::Plan(_))) => self.end_unlaunched(
                spec,
                label,
                execution,
                FailureKind::Cancelled {
                    reason: format!("spawn refused: {e}"),
                },
            ),
            Err(SpawnError::Store(e)) => Err(e),
        }
    }

    /// A candidate that never started ends with `failure`.
    fn end_unlaunched(
        &self,
        spec: &RoundSpec,
        label: &str,
        execution: ExecutionId,
        failure: FailureKind,
    ) -> Result<()> {
        self.emit(
            spec,
            EventKind::CandidateFailed(CandidateFailed {
                label: label.to_string(),
                failure,
            }),
            format!("ended:{}:{label}", spec.round),
            Some(execution),
        )?;
        Ok(())
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
            let record = records.get(&self.key(spec, label));
            self.validate_one(spec, label, record)?;
        }
        Ok(())
    }

    /// Freeze one candidate if it is not frozen yet, then validate it.
    fn validate_one(
        &self,
        spec: &RoundSpec,
        label: &str,
        record: Option<&Execution>,
    ) -> Result<()> {
        let current = self.view(spec)?;
        let c = &current.candidates[label];
        c.worktree.as_ref().context("candidate has no worktree")?;
        c.execution_id
            .as_ref()
            .context("candidate.planned carries no execution id")?;
        let ran = record.is_some_and(|e| {
            !matches!(
                e.status,
                ExecutionStatus::LaunchFailed { .. } | ExecutionStatus::Planned
            )
        });
        if c.frozen.is_none() {
            self.freeze_candidate(spec, &current, label, record, ran)?;
        }
        self.validate_frozen(spec, label, ran)
    }

    /// Freeze a candidate and record its usage; one that never ran keeps its base.
    fn freeze_candidate(
        &self,
        spec: &RoundSpec,
        current: &RoundView,
        label: &str,
        record: Option<&Execution>,
        ran: bool,
    ) -> Result<()> {
        let c = &current.candidates[label];
        let wt = c.worktree.as_ref().context("candidate has no worktree")?;
        let execution = c
            .execution_id
            .clone()
            .context("candidate.planned carries no execution id")?;
        let (head_sha, numstat, diff_digest) = if ran {
            let f = WorktreeManager { git: self.git }.freeze(
                &self.worktree_spec(spec, current, label, &wt.base_sha),
                &execution,
                (self.clock)(),
            )?;
            (f.head_sha, f.numstat, f.diff_digest)
        } else {
            (wt.base_sha.clone(), Vec::new(), sha256_bytes(b""))
        };
        self.record_usage(spec, label, record)?;
        let recorded = self.emit(
            spec,
            EventKind::CandidateFrozen(CandidateFrozen {
                label: label.to_string(),
                head_sha,
                numstat,
                diff_digest,
            }),
            format!("freeze:{}:{label}", spec.round),
            Some(execution),
        )?;
        if recorded {
            self.fault(&format!("{ABORT_AFTER_FREEZE}:{label}"))?;
        }
        Ok(())
    }

    /// Write the usage record of a candidate that has an execution.
    fn record_usage(
        &self,
        spec: &RoundSpec,
        label: &str,
        record: Option<&Execution>,
    ) -> Result<()> {
        let Some(e) = record else { return Ok(()) };
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
        observe::write_usage_record(self.paths, &spec.experiment, &spec.round, &usage)
    }

    /// Validate a frozen candidate that has no report yet.
    fn validate_frozen(&self, spec: &RoundSpec, label: &str, ran: bool) -> Result<()> {
        let view = self.view(spec)?;
        let c = &view.candidates[label];
        if c.validation.is_some() {
            return Ok(());
        }
        let candidate = self.frozen_candidate(&view, label)?;
        let report = self.report_for(&candidate, ran, c.completed.is_some())?;
        let recorded = self.emit(
            spec,
            EventKind::ValidationCompleted(ValidationCompleted {
                label: label.to_string(),
                report,
            }),
            format!("validation:{}:{label}", spec.round),
            Some(candidate.execution_id),
        )?;
        if recorded {
            self.fault(&format!("{ABORT_AFTER_VALIDATION}:{label}"))?;
        }
        Ok(())
    }

    /// The report of a candidate; only a completed one can be eligible.
    fn report_for(
        &self,
        candidate: &FrozenCandidate,
        ran: bool,
        completed: bool,
    ) -> Result<ValidationReport> {
        let mut report = if ran {
            self.validator.validate(candidate)?
        } else {
            not_run_report(candidate, (self.clock)())
        };
        report.eligible = report.eligible && completed;
        Ok(report)
    }

    /// The frozen candidate as the validator takes it.
    fn frozen_candidate(&self, view: &RoundView, label: &str) -> Result<FrozenCandidate> {
        let c = &view.candidates[label];
        let wt = c.worktree.as_ref().context("candidate has no worktree")?;
        let frozen = c.frozen.as_ref().context("candidate not frozen")?;
        Ok(FrozenCandidate {
            label: label.to_string(),
            execution_id: c
                .execution_id
                .clone()
                .context("candidate.planned carries no execution id")?,
            worktree: wt.path.clone(),
            branch: wt.branch.clone(),
            base_sha: wt.base_sha.clone(),
            head_sha: frozen.head_sha.clone(),
            numstat: frozen.numstat.clone(),
            diff_digest: frozen.diff_digest,
            frozen_at: crate::clock::stamp((self.clock)()),
        })
    }

    // ── the decision ────────────────────────────────────────────────────

    /// Advance the round to an outcome. A step that leaves the view as it
    /// was is an error: the next step would do the same, forever.
    fn decide(&self, spec: &RoundSpec) -> Result<RoundOutcome> {
        let mut view = self.view(spec)?;
        loop {
            if let Some(outcome) = self.decide_step(spec, &view)? {
                return Ok(outcome);
            }
            let next = self.view(spec)?;
            if next == view {
                return Err(self.no_progress(spec, &next));
            }
            view = next;
        }
    }

    /// Advance the round one state; `Some` once it has an outcome.
    fn decide_step(&self, spec: &RoundSpec, view: &RoundView) -> Result<Option<RoundOutcome>> {
        match view.state {
            RoundState::JudgingBackground => self.judge(spec).map(Some),
            RoundState::Complete | RoundState::NeedsIntervention => Ok(Some(settled_outcome(view))),
            _ => self.advance_round(spec, view).map(|()| None),
        }
    }

    /// The error of a step that changed nothing: the round, the step, the
    /// view it left, and the events the projection refused (the usual cause).
    fn no_progress(&self, spec: &RoundSpec, view: &RoundView) -> anyhow::Error {
        let refused = match self.recorder.read_all() {
            Ok(read) => fold(&read.events)
                .anomalies
                .iter()
                .map(|a| format!("{}: {}", a.event_id, a.reason))
                .collect::<Vec<_>>()
                .join("; "),
            Err(e) => format!("unreadable: {e:#}"),
        };
        anyhow!(
            "round {} made no progress: the {} step left its view unchanged \
             (state {}, cleaned_from {:?}, final_outcome {:?}); refused events: [{refused}]",
            spec.round,
            step_name(view.state),
            view.state,
            view.cleaned_from,
            view.final_outcome,
        )
    }

    fn advance_round(&self, spec: &RoundSpec, view: &RoundView) -> Result<()> {
        match view.state {
            RoundState::Validating => self.reject_unjudged(spec, view),
            // DECIDED holds a round only without a requested promotion:
            // `winner.selected{requested}` moves on to REVALIDATING at once.
            RoundState::Rejected
            | RoundState::Cleanup
            | RoundState::Decided
            | RoundState::Promoted => self.cleanup(spec, view),
            RoundState::Revalidating | RoundState::Promoting => {
                self.promoter(spec).advance(&spec.round, view).map(drop)
            }
            other => bail!("round {} cannot be decided in state {other}", spec.round),
        }
    }

    /// 0 eligible: the judge is skipped (CMP-14).
    fn reject_unjudged(&self, spec: &RoundSpec, view: &RoundView) -> Result<()> {
        if !view.candidates.values().all(|c| c.validation.is_some()) {
            bail!(
                "round {} is validating with unvalidated candidates",
                spec.round
            );
        }
        self.emit(
            spec,
            EventKind::WinnerRejected(WinnerRejected {
                reason: RejectReason::NoEligible,
            }),
            format!("winner.rejected:{}", spec.round),
            None,
        )?;
        Ok(())
    }

    /// Clean up a round that reached its end. A skipped cleanup stops the
    /// loop: the round would never move on.
    fn cleanup(&self, spec: &RoundSpec, view: &RoundView) -> Result<()> {
        let outcome = RoundCleanup {
            git: self.git,
            recorder: self.recorder,
            paths: self.paths,
            faults: self.faults,
            repo: spec.repo.clone(),
            clock: self.clock,
        }
        .run(
            &spec.round,
            view,
            CleanupOptions {
                prune_branches: spec.config.prune_branches,
                force: false,
            },
        )?;
        if let CleanupOutcome::Skipped { reason } = outcome {
            bail!("{reason}");
        }
        Ok(())
    }

    fn promoter(&self, spec: &RoundSpec) -> RoundPromoter<'_, G> {
        RoundPromoter {
            git: self.git,
            validator: self.validator,
            recorder: self.recorder,
            paths: self.paths,
            faults: self.faults,
            repo: spec.repo.clone(),
            integration_root: integration_root(&spec.worktree_root),
            clock: self.clock,
        }
    }

    /// JUDGING_BACKGROUND: start the judge job, then poll it each tick
    /// until the round is decided (B4, `competition::judging`). `poll` gets
    /// wall-clock time: it compares heartbeats and file times. A judge bundle
    /// that conflicts with one already on disk is never retried: the round
    /// stops in NEEDS_INTERVENTION.
    fn judge(&self, spec: &RoundSpec) -> Result<RoundOutcome> {
        let teammate = self.judge_teammate(spec)?;
        let env = self.judge_env(spec, &teammate);
        match self.run_judge(&env, spec) {
            Ok(()) => self.decide(spec),
            Err(e) => self.judge_error(spec, e),
        }
    }

    /// Start the judge job and poll it until it settles.
    fn run_judge(&self, env: &JudgeEnv, spec: &RoundSpec) -> Result<()> {
        judging::start(env, &spec.round)?;
        while matches!(
            judging::poll(env, &spec.round, Utc::now())?,
            JudgingStatus::Waiting
        ) {
            (self.sleep)(spec.tick);
        }
        Ok(())
    }

    fn judge_teammate(&self, spec: &RoundSpec) -> Result<Teammate> {
        let judge = &spec.config.judge;
        let mut teammate = self.roster.require(JUDGE_TEAMMATE)?.clone();
        teammate.model = Some(judge.model.clone());
        teammate.effort = Some(judge.effort.clone()).filter(|e| !e.is_empty());
        Ok(teammate)
    }

    fn judge_env<'e>(&'e self, spec: &'e RoundSpec, judge: &'e Teammate) -> JudgeEnv<'e> {
        let config = &spec.config;
        JudgeEnv {
            ctx: self.ctx,
            recorder: self.recorder,
            paths: self.paths,
            git: self.git,
            repo: &spec.repo,
            store: self.store,
            judge,
            task_text: &spec.task,
            policy: &config.judge.policy,
            promotion: match &config.promote_to {
                Some(target) => PromotionIntent::Requested {
                    target: target.clone(),
                },
                None => PromotionIntent::NotRequested,
            },
            timeout: Duration::from_secs(config.judge.timeout_s),
            stale_after: heartbeat::STALE_AFTER,
            launcher: self.launcher,
            clock: self.clock,
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

// ── promotion (B5) ──────────────────────────────────────────────────────

/// The parent of a round's temp promotion worktrees: `<worktree root>/_promote`.
/// Labels are plain letters, so no candidate worktree has this name.
pub fn integration_root(worktree_root: &std::path::Path) -> PathBuf {
    worktree_root.join("_promote")
}

/// The round's branch prefix (`mh/exp/<exp8>/...`). A round whose
/// `worktree.created` events exist keeps the prefix they record, so a round
/// started under the old rule (the id's first 8 characters) still freezes;
/// else the random tail of the experiment id.
fn exp8_of(spec: &RoundSpec, view: &RoundView) -> String {
    view.candidates
        .values()
        .filter_map(|c| c.worktree.as_ref())
        .find_map(|wt| wt.branch.strip_prefix("mh/exp/")?.split('/').next())
        .map(str::to_string)
        .unwrap_or_else(|| spec.experiment.short())
}

/// The promotion a round's events ask for: the target branch and the
/// attempt number. The last `operator.promote` wins over the
/// `winner.selected{requested}` target. The attempt is 1, plus 1 per
/// `operator.promote`.
pub(crate) fn promotion_request(
    events: &[EventEnvelope],
    round: &RoundId,
    view: &RoundView,
) -> Result<(String, u32)> {
    let mut target = match view.winner.as_ref().map(|w| &w.promotion) {
        Some(PromotionIntent::Requested { target }) => Some(target.clone()),
        _ => None,
    };
    let mut attempt = 1;
    for e in events {
        if e.round_id.as_ref() != Some(round) {
            continue;
        }
        if let Ok(EventKind::OperatorPromote(p)) = e.event() {
            target = Some(p.target);
            attempt += 1;
        }
    }
    let target = target.with_context(|| format!("round {round} has no promotion target"))?;
    Ok((target, attempt))
}

/// The judged winner as the promotion engine takes it. Its worktree may be
/// gone (a round cleaned up before `promote <round>`): the engine then
/// checks the branch alone.
pub(crate) fn frozen_winner(view: &RoundView, at: DateTime<Utc>) -> Result<FrozenCandidate> {
    let winner = view.winner.as_ref().context("the round has no winner")?;
    let c = view
        .candidates
        .get(&winner.label)
        .with_context(|| format!("the winner {} is not a candidate", winner.label))?;
    let wt = c.worktree.as_ref().context("the winner has no worktree")?;
    let frozen = c.frozen.as_ref().context("the winner was not frozen")?;
    Ok(FrozenCandidate {
        label: winner.label.clone(),
        execution_id: c
            .execution_id
            .clone()
            .context("the winner has no execution id")?,
        worktree: wt.path.clone(),
        branch: wt.branch.clone(),
        base_sha: wt.base_sha.clone(),
        head_sha: frozen.head_sha.clone(),
        numstat: frozen.numstat.clone(),
        diff_digest: frozen.diff_digest,
        frozen_at: crate::clock::stamp(at),
    })
}

/// Promotion of a decided round: what the coordinator and the operator's
/// `promote <round>` share. The caller has brought the round to
/// REVALIDATING (`winner.selected{requested}` or `operator.promote`).
pub(crate) struct RoundPromoter<'a, G: GitClient> {
    pub git: &'a G,
    pub validator: &'a dyn Validator,
    pub recorder: &'a JsonlRecorder,
    pub paths: &'a DatasetPaths,
    pub faults: &'a Faults,
    /// The main repository; the target branch is in it.
    pub repo: PathBuf,
    pub integration_root: PathBuf,
    /// The coordinator's clock.
    pub clock: &'a dyn Fn() -> DateTime<Utc>,
}

impl<G: GitClient> RoundPromoter<'_, G> {
    /// REVALIDATING: run the engine. PROMOTING: finish the publish that a
    /// stopped run started (`resume_promotion`, the target moves at most
    /// once). A fault point returns [`FaultFired`].
    pub(crate) fn advance(&self, round: &RoundId, view: &RoundView) -> Result<PromotionResult> {
        let events = self.recorder.read_all()?.events;
        let (target, attempt) = promotion_request(&events, round, view)?;
        let now = (self.clock)();
        let candidate = frozen_winner(view, now)?;
        let winner = view.winner.as_ref().context("the round has no winner")?;
        let target = BranchRef {
            repo: self.repo.clone(),
            name: target,
        };
        let plan = PromotionPlan {
            experiment: view.experiment_id.clone(),
            round: round.clone(),
            judgment_id: winner.judgment_id.clone(),
            identity: GitIdentity {
                name: FREEZE_NAME.to_owned(),
                email: FREEZE_EMAIL.to_owned(),
                date: crate::clock::stamp(now),
            },
            integration_root: self.integration_root.clone(),
            attempt,
        };
        let validator = DynValidator(self.validator);
        let engine = GitPromotionEngine {
            git: self.git,
            validator: &validator,
            recorder: self.recorder,
            paths: self.paths,
            faults: self.faults,
        };
        match (&view.state, &view.promotion.started) {
            (RoundState::Revalidating, _) => engine.promote(&candidate, &target, &plan),
            (RoundState::Promoting, Some(started)) => {
                engine.resume_promotion(&candidate, &target, &plan, started)
            }
            (state, _) => bail!("round {round} cannot promote in state {state}"),
        }
    }
}

/// The engine takes a sized validator; the coordinator holds a `dyn` one.
struct DynValidator<'a>(&'a dyn Validator);

impl Validator for DynValidator<'_> {
    fn validate(&self, candidate: &FrozenCandidate) -> Result<ValidationReport> {
        self.0.validate(candidate)
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

/// The report of a candidate that never ran: no gates, never eligible.
fn not_run_report(candidate: &FrozenCandidate, now: DateTime<Utc>) -> ValidationReport {
    ValidationReport {
        validation_id: crate::ids::mint_v7(now).to_string(),
        label: candidate.label.clone(),
        head_sha: candidate.head_sha.clone(),
        gates: Vec::new(),
        mechanical_score: 0.0,
        eligible: false,
    }
}

/// The outcome a COMPLETE or NEEDS_INTERVENTION round reports.
/// The step [`Coordinator::advance_round`] takes from `state`.
fn step_name(state: RoundState) -> &'static str {
    match state {
        RoundState::Validating => "reject",
        RoundState::Revalidating | RoundState::Promoting => "promotion",
        _ => "cleanup",
    }
}

fn settled_outcome(view: &RoundView) -> RoundOutcome {
    if view.state == RoundState::NeedsIntervention {
        return RoundOutcome::NeedsIntervention;
    }
    match view.final_outcome {
        Some(FinalOutcome::Rejected) => RoundOutcome::Rejected {
            budget: budget_stopped(view),
        },
        Some(FinalOutcome::NeedsIntervention) => RoundOutcome::NeedsIntervention,
        _ => RoundOutcome::Decided,
    }
}
