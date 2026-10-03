//! `multi-herdr-dataset run` and `resume`: load the config, plan the round,
//! run preflight and refuse (exit 4) before any worktree or model call, then
//! run the round through the coordinator
//! (`horch_core::competition::coordinator`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use horch_core::clock;
use horch_core::competition::budget::UsageMeter;
use horch_core::competition::config::{self, DatasetConfig, JudgeMode, RunFlags, Strategy};
use horch_core::competition::coordinator::{monotonic, Coordinator, RoundOutcome, RoundSpec};
use horch_core::competition::judging::DetachedLauncher;
use horch_core::competition::model::RoundState;
use horch_core::competition::observe::{self, TelemetryUsage};
use horch_core::competition::planner::{plan_round, PlanInput, RoundPlan};
use horch_core::competition::preflight::{evaluate, PreflightCandidate};
use horch_core::competition::promotion::FaultFired;
use horch_core::evaluation::validator::{CommandValidator, GateSpec};
use horch_core::execution::store::ExecutionStore;
use horch_core::fsx;
use horch_core::ids::{ExperimentId, RoundId, TeammateName};
use horch_core::measure::paths::DatasetPaths;
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

pub fn run(ctx: &mut RuntimeContext, env: &BTreeMap<String, String>, args: &RunArgs) -> Result<u8> {
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
pub fn resume(ctx: &mut RuntimeContext, env: &BTreeMap<String, String>, exp: &str) -> Result<u8> {
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
    let (round, plan) = match x.rounds.last() {
        Some(round) => {
            let view = &projection.rounds[round];
            let plan = (view.state == RoundState::Planned)
                .then(|| plan_for(ctx, &config, round, &base_sha))
                .transpose()?;
            (round.clone(), plan)
        }
        None => {
            let round = RoundId::mint(clock::now());
            let plan = plan_for(ctx, &config, &round, &base_sha)?;
            (round, Some(plan))
        }
    };
    coordinate(
        ctx,
        &paths,
        &Round {
            experiment: &experiment,
            round: &round,
            config: &config,
            task: &task,
            safe_n: report.safe_n,
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
    // absolute. A relative root is relative to the project.
    let root = match config.worktree_root.clone() {
        Some(root) => root,
        None => paths.default_worktree_root(experiment)?,
    };
    config.worktree_root = Some(if root.is_absolute() {
        root
    } else {
        project.join(root)
    });

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
    if !failed.is_empty() {
        println!(
            "REFUSED: preflight failed ({}). No worktree was created and no model was called.",
            failed.join(", ")
        );
        return Ok(exit::PREFLIGHT_FAILED);
    }
    ctx.settings.faults.abort_if("abort-after-preflight");
    if plan.candidates.is_empty() {
        println!("REFUSED: no candidate can run within the usage limits.");
        return Ok(exit::BUDGET_REFUSED);
    }
    coordinate(
        ctx,
        paths,
        &Round {
            experiment,
            round: &round_id,
            config: &config,
            task,
            safe_n: report.safe_n,
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
        })
        .collect();
    // `fail-gate:<name>` reaches the gates from `run` (and from promotion's
    // revalidation, which uses this validator too).
    let validator = CommandValidator::new(gates, artifacts, ctx.settings.faults.points().clone());
    let usage = TelemetryUsage {
        locations: Locations::from_context(ctx),
    };
    let meter = UsageMeter::default();
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
    let watch_command = PaneShell::host().command_line(
        &exe,
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
    let (line, code) = outcome_line(&outcome, &view);
    println!("round {} {line}", r.round);
    Ok(code)
}

/// The last line `run`, `resume` and `promote` print, and the exit code:
/// DECIDED, PROMOTED and COMPLETE 0; the budget 3; NEEDS_INTERVENTION 5;
/// REJECTED 6.
pub(super) fn outcome_line(outcome: &RoundOutcome, view: &RoundView) -> (String, u8) {
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
            match &view.needs_intervention {
                Some(n) => format!("NEEDS_INTERVENTION: {}", n.reason),
                None => "NEEDS_INTERVENTION".to_string(),
            },
            exit::NEEDS_INTERVENTION,
        ),
    }
}

/// How often the coordinator looks at its candidates.
const TICK: Duration = Duration::from_millis(500);

fn load_roster(ctx: &RuntimeContext) -> Result<Roster> {
    Roster::load_layered(
        ctx.inherited.home_var.as_deref().map(Path::new),
        ctx.bins.roster_override.as_deref(),
        None,
    )
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
pub struct SavedRun {
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
