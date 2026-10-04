//! `multi-herdr-dataset run` and `resume`: load the config, plan the round,
//! run preflight and refuse (exit 4) before any worktree or model call, then
//! run the round through the coordinator
//! (`horch_core::competition::coordinator`).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use horch_core::clock;
use horch_core::competition::budget::{resolve_estimate, MeasuredTokens, UsageMeter};
use horch_core::competition::config::{
    self, DatasetConfig, ExpectedTokens, JudgeMode, RunFlags, Strategy,
};
use horch_core::competition::coordinator::{monotonic, Coordinator, RoundOutcome, RoundSpec};
use horch_core::competition::judging::DetachedLauncher;
use horch_core::competition::model::RoundState;
use horch_core::competition::observe::{self, TelemetryUsage};
use horch_core::competition::planner::{plan_round, PlanInput, RoundPlan};
use horch_core::competition::preflight::{
    evaluate, recorded_estimates, PreflightCandidate, PreflightReport, TokenEstimate,
};
use horch_core::competition::promotion::FaultFired;
use horch_core::evaluation::validator::{CommandValidator, GateSpec};
use horch_core::execution::store::ExecutionStore;
use horch_core::fsx;
use horch_core::ids::{ExperimentId, RoundId, TeammateName};
use horch_core::measure::event::EventEnvelope;
use horch_core::measure::paths::{component, DatasetPaths};
use horch_core::measure::projection::{fold, RoundView};
use horch_core::measure::recorder::JsonlRecorder;
use horch_core::measure::redact::redact;
use horch_core::measure::store::{self, StoreOptions};
use horch_core::roster::Roster;
use horch_core::routing::eligible::EligibilityFilter;
use horch_core::routing::policy::Policy;
use horch_core::routing::quota::QuotaView;
use horch_core::routing::snapshot::{obtain, QuotaEnv};
use horch_core::runtime::fault::ABORT_EXIT_CODE;
use horch_core::runtime::machine::{self, Known};
use horch_core::runtime::RuntimeContext;
use horch_core::usage::Locations;
use horch_core::vcs::git::GitCli;
use horch_core::vcs::worktree::WorktreeSpec;
use horch_core::workspace::herdr::Herdr;
use horch_core::workspace::paneshell::PaneShell;
use serde::{Deserialize, Serialize};

use super::cli::RunArgs;
use super::preflight::{self, ExperimentFacts, GatherInput};
use super::{dataset_paths, exit};

pub(crate) fn run(
    ctx: &mut RuntimeContext,
    env: &BTreeMap<String, String>,
    args: &RunArgs,
) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let experiment = ExperimentId::mint(clock::now());
    let saved = SavedRun::from_args(args);
    saved.write(&paths, &experiment)?;
    preflight_and_run(ctx, env, &paths, &experiment, &args.task, &saved)
}

/// `resume <exp>`: re-enter the experiment where its events stop. Preflight
/// runs again when it never completed; a planned round is planned again with
/// its own id; a running round is driven on, adopting every candidate that a
/// killed coordinator started.
pub(crate) fn resume(
    ctx: &mut RuntimeContext,
    env: &BTreeMap<String, String>,
    exp: &str,
) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let experiment = ExperimentId::new(exp)?;
    let projection = fold(&store::read_all(&paths)?.events);
    let Some(x) = projection.experiments.get(&experiment) else {
        // Killed before `experiment.created`: start over under the same id.
        let saved = SavedRun::read(&paths, &experiment)?;
        return preflight_and_run(ctx, env, &paths, &experiment, &saved.task, &saved);
    };
    match x.state {
        RoundState::Aborted => {
            println!("experiment {experiment} was aborted by preflight");
            return Ok(exit::PREFLIGHT_FAILED);
        }
        RoundState::Preflight if x.preflight.is_none() => {
            let saved = SavedRun::read(&paths, &experiment)?;
            return preflight_and_run(ctx, env, &paths, &experiment, &saved.task, &saved);
        }
        RoundState::Preflight => {
            bail!("experiment {experiment} failed preflight but was not aborted")
        }
        _ => {}
    }
    let report = x
        .preflight
        .clone()
        .context("the experiment has no preflight report")?;
    let base_sha = x.created.base_sha.clone();
    let config = manifest_config(&paths, &experiment)?;
    let task = SavedRun::read(&paths, &experiment)?.task;
    let (round, plan, mut planned_models) = match x.rounds.last() {
        Some(round) => {
            let view = &projection.rounds[round];
            let plan = (view.state == RoundState::Planned)
                .then(|| plan_for(ctx, &config, round, &base_sha))
                .transpose()?;
            let models = view
                .candidates
                .iter()
                .filter_map(|(l, c)| Some((l.clone(), c.planned.as_ref()?.model.to_string())))
                .collect();
            (round.clone(), plan, models)
        }
        None => {
            let round = RoundId::mint(clock::now());
            let plan = plan_for(ctx, &config, &round, &base_sha)?;
            (round, Some(plan), Vec::new())
        }
    };
    if let Some(p) = &plan {
        planned_models.extend(
            p.candidates
                .iter()
                .map(|c| (c.label.to_string(), c.model.to_string())),
        );
    }
    let estimates = resumed_estimates(&report, &planned_models, &config.budget.expected_tokens);
    coordinate(
        ctx,
        &paths,
        &Round {
            experiment: &experiment,
            round: &round,
            config: &config,
            task: &task,
            safe_n: report.safe_n,
            estimates,
        },
        plan.as_ref(),
    )
}

/// Preflight, then the round. `run` calls it for a new experiment, and
/// `resume` for one whose preflight never completed.
fn preflight_and_run(
    ctx: &mut RuntimeContext,
    env: &BTreeMap<String, String>,
    paths: &DatasetPaths,
    experiment: &ExperimentId,
    task: &str,
    saved: &SavedRun,
) -> Result<u8> {
    let project = ctx.paths.project()?;
    let flags = saved.flags()?;
    let mut config = config::load(&project, &flags)?;
    let now = clock::now();
    let round_id = RoundId::mint(now);

    // PRE-13 checks the root against the trusted directories, so it must be
    // absolute. A relative root is relative to the project. Each experiment
    // gets its own directory under the root, as under the default root, so
    // the worktrees an earlier round kept never collide with this one's.
    config.worktree_root = Some(experiment_worktree_root(
        paths,
        &project,
        experiment,
        config.worktree_root.as_deref(),
    )?);

    let git = GitCli::new(ctx.bins.harness.git.clone());
    let head = preflight::repo_facts(&git, &project, &[], None).base_sha;
    let (plan, view) = plan_with_view(ctx, &config, &round_id, head.as_deref().unwrap_or(""))?;

    let branches = planned_branches(experiment, &plan, &config);
    let git_facts = preflight::repo_facts(&git, &project, &branches, config.promote_to.as_deref());
    let candidates = plan
        .candidates
        .iter()
        .map(|c| PreflightCandidate {
            label: c.label.to_string(),
            teammate: c.teammate.clone(),
            harness: c.harness,
            model: c.model.clone(),
            effort: c.effort.clone(),
        })
        .collect();
    let (pre_plan, snapshot) = preflight::gather(
        ctx,
        GatherInput {
            paths,
            task,
            config: &config,
            candidates,
            git: git_facts,
            view: &view,
        },
    );
    let report = evaluate(&pre_plan, &snapshot);

    let recorder = JsonlRecorder::open(paths, StoreOptions::from_faults(&ctx.settings.faults))?;
    let failed = preflight::record(
        &recorder,
        paths,
        &ExperimentFacts {
            experiment,
            task,
            plan: &pre_plan,
            report: &report,
            env: snapshot_env(ctx, env),
            at: now,
        },
        &ctx.settings.faults,
    )?;
    drop(recorder);

    println!("experiment {experiment}");
    print!("{}", render_plan(&plan));
    print!("{}", preflight::render(&report));
    // An empty plan fails PRE-08, but the planner refused every candidate
    // (excluded, or over its usage limits): that is the budget/quota
    // refusal, exit 3 (dataset design §6 B2), not a failed machine check.
    // The report is recorded and the experiment aborted all the same.
    if plan.candidates.is_empty() && failed.iter().all(|id| id == "PRE-08") {
        println!(
            "REFUSED: no candidate can run within the usage limits. \
             No worktree was created and no model was called."
        );
        return Ok(exit::BUDGET_REFUSED);
    }
    if !failed.is_empty() {
        println!(
            "REFUSED: preflight failed ({}). No worktree was created and no model was called.",
            failed.join(", ")
        );
        return Ok(exit::PREFLIGHT_FAILED);
    }
    ctx.settings.faults.abort_if("abort-after-preflight");
    coordinate(
        ctx,
        paths,
        &Round {
            experiment,
            round: &round_id,
            config: &config,
            task,
            safe_n: report.safe_n,
            estimates: meter_estimates(
                pre_plan
                    .candidates
                    .iter()
                    .map(|c| (c.label.as_str(), c.model.as_str())),
                &pre_plan.expected_tokens,
                &config.budget.expected_tokens,
                &pre_plan.measured_tokens,
            ),
        },
        Some(&plan),
    )
}

/// The round a coordinator runs.
struct Round<'a> {
    experiment: &'a ExperimentId,
    round: &'a RoundId,
    config: &'a DatasetConfig,
    task: &'a str,
    safe_n: u32,
    /// The live meter's expected tokens per model ([`meter_estimates`]).
    estimates: BTreeMap<String, TokenEstimate>,
}

/// The expected tokens of each candidate model for the live meter, resolved
/// as PRE-09 resolves them ([`resolve_estimate`]). The coordinator asks the
/// meter by model, not by label: 2 labels of 1 model with different
/// estimates get the per-kind maximum of both.
fn meter_estimates<'a>(
    candidates: impl Iterator<Item = (&'a str, &'a str)>,
    per_label: &BTreeMap<String, TokenEstimate>,
    configured: &ExpectedTokens,
    measured: &BTreeMap<String, MeasuredTokens>,
) -> BTreeMap<String, TokenEstimate> {
    let mut out: BTreeMap<String, TokenEstimate> = BTreeMap::new();
    for (label, model) in candidates {
        let (e, _) = resolve_estimate(model, per_label.get(label).copied(), configured, measured);
        out.entry(model.to_string())
            .and_modify(|m| {
                *m = TokenEstimate {
                    input: m.input.max(e.input),
                    cache_write_5m: m.cache_write_5m.max(e.cache_write_5m),
                    cache_write_1h: m.cache_write_1h.max(e.cache_write_1h),
                    cache_read: m.cache_read.max(e.cache_read),
                    output: m.output.max(e.output),
                }
            })
            .or_insert(e);
    }
    out
}

/// The live meter's estimates for a resumed round: the tokens PRE-09
/// recorded for each label ([`recorded_estimates`]) while the label keeps
/// its model, else `budget.expected_tokens` and the default.
fn resumed_estimates(
    report: &PreflightReport,
    planned_models: &[(String, String)],
    configured: &ExpectedTokens,
) -> BTreeMap<String, TokenEstimate> {
    let recorded = recorded_estimates(report);
    let per_label = planned_models
        .iter()
        .filter_map(|(label, model)| {
            let r = recorded.get(label).filter(|r| &r.model == model)?;
            Some((label.clone(), r.tokens))
        })
        .collect();
    meter_estimates(
        planned_models.iter().map(|(l, m)| (l.as_str(), m.as_str())),
        &per_label,
        configured,
        &BTreeMap::new(),
    )
}

/// Run the round through the coordinator and map where it ended to an exit
/// code: 0 decided, 3 the budget stopped it, 5 needs the operator, 6
/// rejected. The coordinator waits for the judge job. A fault point aborts the process.
fn coordinate(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    r: &Round,
    plan: Option<&RoundPlan>,
) -> Result<u8> {
    let project = ctx.paths.project()?;
    // The service runs `<exe> worker <role>` in each pane: that is horch,
    // not this binary.
    let mut spawn_ctx = ctx.clone();
    spawn_ctx.bins.current_exe = Some(ctx.bins.horch_exe.clone());
    let recorder = JsonlRecorder::open(paths, StoreOptions::from_faults(&ctx.settings.faults))?;
    let git = GitCli::new(ctx.bins.harness.git.clone());
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let store = ExecutionStore::open_in(ctx)?;
    let roster = load_roster(ctx)?;
    let artifacts = paths
        .artifacts_dir(r.experiment, r.round)?
        .join("validation");
    observe::ensure_dirs(paths, &artifacts)?;
    let gates = r
        .config
        .gates
        .iter()
        .map(|g| GateSpec {
            name: g.name.clone(),
            command: g.command.clone(),
            timeout: Duration::from_secs(g.timeout_s),
            required: g.required,
        })
        .collect();
    // `fail-gate:<name>` reaches the gates from `run` (and from promotion's
    // revalidation, which uses this validator too).
    let validator = CommandValidator::new(gates, artifacts, ctx.settings.faults.points().clone());
    let usage = TelemetryUsage {
        locations: Locations::from_context(ctx),
    };
    let meter = UsageMeter {
        estimates: r.estimates.clone(),
        ..UsageMeter::default()
    };
    let worktree_root = r
        .config
        .worktree_root
        .clone()
        .context("the config has no worktree root")?;
    let disk_dir = existing_ancestor(&worktree_root);
    let machine_file = ctx.settings.machine_file.clone();
    let disk_free = move || match machine::probe(
        &disk_dir,
        &machine::ProbeBins::default(),
        machine_file.as_deref(),
    )
    .disk_free_bytes
    {
        Known::Known(free) => Some(free),
        Known::Unknown => None,
    };
    let clock = monotonic(clock::now);
    let sleep = |d: Duration| std::thread::sleep(d);
    let exe = ctx.bins.exe()?;
    let data_root = ctx.paths.data_root.to_string_lossy();
    let watch_command = PaneShell::host().command_line_with_env(
        &exe,
        &[("HORCH_DATA_DIR", data_root.as_ref())],
        &[
            "watch".to_string(),
            r.experiment.to_string(),
            "--state-dir".to_string(),
            ctx.paths.state_root.to_string_lossy().into_owned(),
            "--project".to_string(),
            project.to_string_lossy().into_owned(),
        ],
    );
    let coordinator = Coordinator {
        ctx: &spawn_ctx,
        recorder: &recorder,
        paths,
        git: &git,
        workspace: &herdr,
        store: &store,
        roster: &roster,
        validator: &validator,
        usage: &usage,
        meter: &meter,
        disk_free: &disk_free,
        clock: &clock,
        sleep: &sleep,
        faults: &ctx.settings.faults,
        launcher: &DetachedLauncher(ctx),
    };
    let spec = RoundSpec {
        experiment: r.experiment.clone(),
        round: r.round.clone(),
        index: 0,
        config: r.config.clone(),
        task: r.task.to_string(),
        repo: project,
        worktree_root,
        safe_n: r.safe_n,
        tick: TICK,
        watch_command,
    };
    let outcome = match plan {
        Some(plan) => coordinator.start(&spec, plan),
        None => coordinator.drive(&spec),
    };
    let outcome = match outcome {
        Ok(o) => o,
        Err(e) => {
            if let Some(f) = e.downcast_ref::<FaultFired>() {
                eprintln!("multi-herdr-dataset: {f}: aborting");
                std::process::exit(ABORT_EXIT_CODE);
            }
            return Err(e);
        }
    };
    coordinator.close_workspace(&spec);
    let view = coordinator.view(&spec)?;
    let events = store::read_all(paths)?.events;
    let reason = latest_intervention(&events, r.round);
    let (line, code) = outcome_line(&outcome, &view, reason.as_deref());
    println!("round {} {line}", r.round);
    Ok(code)
}

/// The last line `run`, `resume` and `promote` print, and the exit code:
/// DECIDED, PROMOTED and COMPLETE 0; the budget 3; NEEDS_INTERVENTION 5;
/// REJECTED 6.
///
/// `reason` is the [`latest_intervention`] of the round.
pub(super) fn outcome_line(
    outcome: &RoundOutcome,
    view: &RoundView,
    reason: Option<&str>,
) -> (String, u8) {
    match outcome {
        RoundOutcome::Decided => {
            let p = &view.promotion;
            let line = match (&p.started, &p.completed, &view.winner) {
                (Some(started), Some(done), _) => {
                    format!("PROMOTED: {} is at {}", started.target, done.dest_after)
                }
                (_, _, Some(w)) => format!("DECIDED: winner {}", w.label),
                _ => "DECIDED".to_string(),
            };
            (line, exit::SUCCESS)
        }
        RoundOutcome::Rejected { budget: true } => (
            "REJECTED: the budget stopped the candidates".to_string(),
            exit::BUDGET_REFUSED,
        ),
        RoundOutcome::Rejected { budget: false } => (
            match view.rejected {
                Some(reason) => format!("REJECTED: {reason}"),
                None => "REJECTED".to_string(),
            },
            exit::REJECTED,
        ),
        RoundOutcome::NeedsIntervention => (
            match reason.or(view.needs_intervention.as_ref().map(|n| n.reason.as_str())) {
                Some(reason) => format!("NEEDS_INTERVENTION: {reason}"),
                None => "NEEDS_INTERVENTION".to_string(),
            },
            exit::NEEDS_INTERVENTION,
        ),
    }
}

/// How often the coordinator looks at its candidates.
const TICK: Duration = Duration::from_millis(500);

/// Why the round last stopped for the operator: its latest
/// `round.needs_intervention` or `promotion.conflicted`, whichever came
/// last. The projection keeps each kind apart, so after a second `promote`
/// it alone cannot tell a new conflict from the first attempt's reason.
fn latest_intervention(events: &[EventEnvelope], round: &RoundId) -> Option<String> {
    let mut target: Option<String> = None;
    let mut reason = None;
    for e in events.iter().filter(|e| e.round_id.as_ref() == Some(round)) {
        match e.kind.as_str() {
            "operator.promote" => {
                target = e.payload["target"].as_str().map(str::to_string);
            }
            "round.needs_intervention" => {
                reason = e.payload["reason"].as_str().map(str::to_string);
            }
            "promotion.conflicted" => {
                let paths: Vec<&str> = e.payload["paths"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|p| p.as_str()).collect())
                    .unwrap_or_default();
                let onto = target.as_deref().unwrap_or("the target");
                reason = Some(format!(
                    "the winner conflicts with {onto} in {}",
                    paths.join(", ")
                ));
            }
            _ => {}
        }
    }
    reason
}

fn load_roster(ctx: &RuntimeContext) -> Result<Roster> {
    let roster = Roster::load_layered(
        ctx.inherited.home_var.as_deref().map(Path::new),
        ctx.bins.roster_override.as_deref(),
        None,
    )?;
    warn_once(&roster, &ROSTER_WARNED, &mut std::io::stderr());
    Ok(roster)
}

/// Set once the roster warnings are printed: `run` loads the roster more
/// than once.
static ROSTER_WARNED: AtomicBool = AtomicBool::new(false);

/// Write each [`Roster::load_warnings`] line to `out` as `warning: <line>`,
/// as `horch`'s `load_roster` does, unless `warned` is already set.
fn warn_once(roster: &Roster, warned: &AtomicBool, out: &mut impl Write) {
    if warned.swap(true, Ordering::Relaxed) {
        return;
    }
    for w in roster.load_warnings() {
        let _ = writeln!(out, "warning: {w}");
    }
}

/// Plan round `round_id` from `base_sha`. No quota probe: planning makes no
/// call that could cost a model turn.
fn plan_with_view(
    ctx: &RuntimeContext,
    config: &DatasetConfig,
    round_id: &RoundId,
    base_sha: &str,
) -> Result<(RoundPlan, QuotaView)> {
    let roster = load_roster(ctx)?;
    let policy = Policy::load(
        &ctx.paths.state_root,
        ctx.settings.balance_override.as_deref(),
    )?;
    let view = obtain(
        &ctx.paths.state_root,
        clock::now(),
        &policy,
        false,
        &QuotaEnv::from_context(ctx),
    )?;
    let plan = plan_round(&PlanInput {
        round_id,
        index: 0,
        base_sha,
        n: config.candidates,
        baseline: config.baseline.clone(),
        roster: &roster,
        view: &view,
        filter: &EligibilityFilter::default(),
        config,
    });
    Ok((plan, view))
}

fn plan_for(
    ctx: &RuntimeContext,
    config: &DatasetConfig,
    round_id: &RoundId,
    base_sha: &str,
) -> Result<RoundPlan> {
    Ok(plan_with_view(ctx, config, round_id, base_sha)?.0)
}

/// The config preflight recorded in the manifest.
fn manifest_config(paths: &DatasetPaths, exp: &ExperimentId) -> Result<DatasetConfig> {
    let file = paths.manifest(exp)?;
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&file).with_context(|| format!("reading {}", file.display()))?,
    )?;
    serde_json::from_value(manifest["config"].clone())
        .with_context(|| format!("the config in {}", file.display()))
}

/// `path`, or its nearest ancestor that exists.
fn existing_ancestor(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|p| p.exists())
        .unwrap_or(path)
        .to_path_buf()
}

/// What `run` was asked, kept in `experiments/<exp>/run.json` (0600) before
/// any event, so `resume` can run preflight again. The task is redacted
/// (SEC-01): a resumed round briefs its candidates with the redacted text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SavedRun {
    pub task: String,
    pub candidates: Option<u32>,
    pub strategy: Option<Strategy>,
    pub budget_usd: Option<String>,
    pub judge: Option<JudgeMode>,
    pub baseline: Option<String>,
    pub promote_to: Option<String>,
    pub worktree_root: Option<PathBuf>,
    pub allow_dirty: bool,
}

impl SavedRun {
    fn from_args(args: &RunArgs) -> Self {
        SavedRun {
            task: redact(&args.task).into_owned(),
            candidates: args.candidates,
            strategy: args.strategy,
            budget_usd: args.budget_usd.clone(),
            judge: args.judge,
            baseline: args.baseline.clone(),
            promote_to: args.promote_to.clone(),
            worktree_root: args.worktree_root.clone(),
            allow_dirty: args.allow_dirty,
        }
    }

    fn flags(&self) -> Result<RunFlags> {
        Ok(RunFlags {
            task: self.task.clone(),
            candidates: self.candidates,
            strategy: self.strategy,
            budget_usd: self.budget_usd.clone(),
            judge: self.judge,
            baseline: self
                .baseline
                .as_deref()
                .map(TeammateName::new)
                .transpose()?,
            promote_to: self.promote_to.clone(),
            worktree_root: self.worktree_root.clone(),
            allow_dirty: self.allow_dirty,
        })
    }

    fn file(paths: &DatasetPaths, exp: &ExperimentId) -> Result<PathBuf> {
        Ok(paths.experiment_dir(exp)?.join("run.json"))
    }

    fn write(&self, paths: &DatasetPaths, exp: &ExperimentId) -> Result<()> {
        paths.ensure()?;
        let file = Self::file(paths, exp)?;
        observe::ensure_dirs(paths, file.parent().context("run file")?)?;
        fsx::create_immutable(&file, &serde_json::to_vec_pretty(self)?, 0o600)
            .with_context(|| format!("writing {}", file.display()))?;
        Ok(())
    }

    fn read(paths: &DatasetPaths, exp: &ExperimentId) -> Result<SavedRun> {
        let file = Self::file(paths, exp)?;
        let bytes = std::fs::read(&file)
            .with_context(|| format!("no experiment {exp}: reading {}", file.display()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}

/// Where `experiment`'s worktrees go: `<root>/<experiment>` for an explicit
/// `--worktree-root` (relative to `project` when relative), else the
/// default root, which is per experiment already.
fn experiment_worktree_root(
    paths: &DatasetPaths,
    project: &Path,
    experiment: &ExperimentId,
    explicit: Option<&Path>,
) -> Result<PathBuf> {
    let Some(root) = explicit else {
        return Ok(paths.default_worktree_root(experiment)?);
    };
    let root = if root.is_absolute() {
        root.to_path_buf()
    } else {
        project.join(root)
    };
    Ok(root.join(component("experiment id", experiment.as_str())?))
}

/// The branch each planned candidate gets: `mh/exp/<exp8>/r<idx>/<label>`.
fn planned_branches(
    experiment: &ExperimentId,
    plan: &RoundPlan,
    config: &DatasetConfig,
) -> Vec<String> {
    let root = config.worktree_root.clone().unwrap_or_default();
    plan.candidates
        .iter()
        .map(|c| {
            WorktreeSpec {
                repo: root.clone(),
                root: root.clone(),
                exp8: experiment.short(),
                round_index: plan.index,
                label: c.label.to_string(),
                base_sha: plan.base_sha.clone(),
            }
            .branch()
        })
        .collect()
}

/// The variables `envsnap` may keep: the plain ones the binary read, plus
/// the `HORCH_*` settings this context holds.
fn snapshot_env(ctx: &RuntimeContext, env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = env.clone();
    if let Some(balance) = &ctx.settings.balance_override {
        out.insert("HORCH_BALANCE".to_string(), balance.clone());
    }
    if let Some(dir) = &ctx.bins.roster_override {
        out.insert(
            "HORCH_TEAMMATES_DIR".to_string(),
            dir.to_string_lossy().into_owned(),
        );
    }
    for (key, value) in ctx.bins.overrides.env_pairs() {
        out.insert(key.to_string(), value.to_string_lossy().into_owned());
    }
    out
}

fn render_plan(plan: &RoundPlan) -> String {
    let mut out = format!(
        "round {} plan ({} candidates):\n",
        plan.index,
        plan.candidates.len()
    );
    for c in &plan.candidates {
        out.push_str(&format!(
            "  {} {:?} {} p={:.3}\n",
            c.label, c.slot, c.config_id, c.propensity
        ));
    }
    if let Some(reason) = plan.baseline.missing_reason() {
        out.push_str(&format!("  no baseline: {reason}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The meter gets PRE-09's estimate per model: the plan's per-label value
    /// first, then the config; 2 labels of 1 model keep the per-kind maximum.
    /// A model with no source gets the default.
    #[test]
    fn meter_estimates_follow_pre_09_per_model() {
        let est = |input, output| TokenEstimate {
            input,
            output,
            ..TokenEstimate::default()
        };
        let configured = ExpectedTokens {
            all: Some(est(10, 10)),
            models: [("opus".to_string(), est(50, 5))].into(),
        };
        let per_label = [("B".to_string(), est(1, 99))].into();
        let got = meter_estimates(
            [("A", "sonnet"), ("B", "sonnet"), ("C", "opus")].into_iter(),
            &per_label,
            &configured,
            &BTreeMap::new(),
        );
        assert_eq!(got["sonnet"], est(10, 99));
        assert_eq!(got["opus"], est(50, 5));
        let none = meter_estimates(
            [("A", "sonnet")].into_iter(),
            &BTreeMap::new(),
            &ExpectedTokens::default(),
            &BTreeMap::new(),
        );
        let (default, source) =
            resolve_estimate("sonnet", None, &ExpectedTokens::default(), &BTreeMap::new());
        assert_eq!(
            source,
            horch_core::competition::budget::EstimateSource::Default
        );
        assert_eq!(none["sonnet"], default);
    }

    /// G9: a resumed round's meter uses the tokens PRE-09 recorded for each
    /// label, read back from the `preflight.completed` event, not the
    /// default. A label whose model changed gets the config or the default.
    #[test]
    fn resume_meter_uses_the_recorded_estimates() {
        use horch_core::measure::event::{Actor, EventKind, PreflightCompleted};
        use horch_core::measure::recorder::{NewEvent, Recorder};

        let measured = TokenEstimate {
            input: 2_000,
            cache_read: 30_000,
            output: 500,
            ..TokenEstimate::default()
        };
        let report: PreflightReport = serde_json::from_value(serde_json::json!({
            "schema_version": "1.0.0",
            "checks": [{"id": "PRE-09", "status": "pass", "detail": "",
                        "measured": {"estimate_source": {"A": "measured 3 runs", "B": "default"},
                                     "estimates": {"A": {"model": "sonnet", "tokens": measured},
                                                   "B": {"model": "opus", "tokens": measured}}}}],
            "safe_n": 2, "waves": 1, "projected_cost_microusd": 0,
            "machine": {"os": "macos", "arch": "aarch64", "cpus": 10,
                        "mem_total_bytes": null, "mem_available_bytes": null,
                        "disk_free_bytes": null, "disk_total_bytes": null,
                        "gpu": "apple_silicon", "max_open_files": null, "max_processes": null},
            "environment_digest": horch_core::measure::digest::sha256_bytes(b"e").to_string(),
            "passed": true,
        }))
        .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let paths = DatasetPaths::from_slug(tmp.path(), "fixture");
        let exp = ExperimentId::new("01a10753-3c91-7bd1-99e8-70f01d2b52d5").unwrap();
        let rec = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();
        rec.append(NewEvent {
            kind: EventKind::PreflightCompleted(PreflightCompleted { report }),
            actor: Actor::Coordinator,
            experiment_id: exp.clone(),
            round_id: None,
            execution_id: None,
            idempotency_key: "preflight.completed:x".into(),
            occurred_at: clock::now(),
        })
        .unwrap();
        drop(rec);
        let events = store::read_all(&paths).unwrap().events;
        let report = events
            .iter()
            .find_map(|e| match e.event() {
                Ok(EventKind::PreflightCompleted(p)) => Some(p.report),
                _ => None,
            })
            .unwrap();

        // B was planned on opus; the resumed plan puts it on codex-sol.
        let planned = [
            ("A".to_string(), "sonnet".to_string()),
            ("B".to_string(), "codex-sol".to_string()),
        ];
        let got = resumed_estimates(&report, &planned, &ExpectedTokens::default());
        assert_eq!(got["sonnet"], measured);
        let (default, _) = resolve_estimate(
            "codex-sol",
            None,
            &ExpectedTokens::default(),
            &BTreeMap::new(),
        );
        assert_eq!(got["codex-sol"], default);
        assert_ne!(measured, default);
    }

    /// F7 (roster-resilience): a teammate file that does not parse gives one
    /// `warning:` line, and a second roster load prints nothing more.
    #[test]
    fn roster_warnings_print_once() {
        let tmp = tempfile::tempdir().unwrap();
        let opus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates/opus.md");
        let text = std::fs::read_to_string(opus)
            .unwrap()
            .replace("name: opus", "name: newcomer")
            .replacen("\n---\n", "\nrequires: [no-such-tool]\n---\n", 1);
        std::fs::write(tmp.path().join("newcomer.md"), text).unwrap();
        let roster = Roster::load_layered(None, Some(tmp.path()), None).unwrap();
        let warned = AtomicBool::new(false);
        let mut out = Vec::new();
        warn_once(&roster, &warned, &mut out);
        warn_once(&roster, &warned, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.starts_with("warning: teammate 'newcomer'"), "{text}");
    }

    fn event(round: &RoundId, kind: &str, payload: serde_json::Value) -> EventEnvelope {
        serde_json::from_value(serde_json::json!({
            "schema_version": "1.0.0",
            "event_id": "01a10754-0000-7000-8000-000000000001",
            "kind": kind,
            "occurred_at": "2026-10-04T14:32:14.000Z",
            "actor": "operator",
            "experiment_id": "01a10753-3c91-7bd1-99e8-70f01d2b52d5",
            "round_id": round.as_str(),
            "idempotency_key": format!("{kind}:x"),
            "payload": payload,
        }))
        .unwrap()
    }

    /// LA-9: a second `promote` that conflicts printed the first attempt's
    /// dirty-checkout reason. The latest stop wins.
    #[test]
    fn the_latest_intervention_names_a_later_conflict() {
        let round = RoundId::new("01a10753-3ca5-7399-b6b4-1dabda77e8db").unwrap();
        let other = RoundId::new("01a10753-3ca5-7399-b6b4-1dabda77e8dc").unwrap();
        let dirty = "refs/heads/la/t-dirty is checked out with local changes";
        let mut events = vec![
            event(
                &round,
                "operator.promote",
                serde_json::json!({"target": "la/t-dirty"}),
            ),
            event(
                &round,
                "round.needs_intervention",
                serde_json::json!({"reason": dirty, "source": "promotion"}),
            ),
        ];
        assert_eq!(latest_intervention(&events, &round).as_deref(), Some(dirty));
        events.push(event(
            &round,
            "operator.promote",
            serde_json::json!({"target": "la/t-moved"}),
        ));
        events.push(event(
            &round,
            "promotion.conflicted",
            serde_json::json!({"paths": ["textutil.py"]}),
        ));
        events.push(event(
            &other,
            "round.needs_intervention",
            serde_json::json!({"reason": "another round", "source": "operator"}),
        ));
        assert_eq!(
            latest_intervention(&events, &round).as_deref(),
            Some("the winner conflicts with la/t-moved in textutil.py")
        );
        assert_eq!(latest_intervention(&events[..0], &round), None);
    }

    /// LA-11: with `--worktree-root`, a second experiment collided with the
    /// worktrees an earlier NEEDS_INTERVENTION round kept at `<root>/<label>`.
    #[test]
    fn an_explicit_worktree_root_is_per_experiment() {
        let paths = DatasetPaths::new(Path::new("/state"), Path::new("/proj"));
        let one = ExperimentId::new("01a1073d-cb8c-759c-8334-e02289432c6b").unwrap();
        let two = ExperimentId::new("01a1073f-5714-7933-8a6a-cd76f13cbd35").unwrap();
        let root = |exp: &ExperimentId, explicit: Option<&str>| {
            experiment_worktree_root(&paths, Path::new("/proj"), exp, explicit.map(Path::new))
                .unwrap()
        };
        assert_eq!(
            root(&one, Some("/wt")),
            PathBuf::from("/wt/01a1073d-cb8c-759c-8334-e02289432c6b")
        );
        assert_ne!(root(&one, Some("/wt")), root(&two, Some("/wt")));
        assert_eq!(
            root(&one, Some("wt")),
            PathBuf::from("/proj/wt/01a1073d-cb8c-759c-8334-e02289432c6b")
        );
        assert_eq!(root(&one, None), paths.default_worktree_root(&one).unwrap());
    }
}
