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
use horch_core::competition::coordinator::{
    dataset_workspace, monotonic, record_round_plan, workspace_name, Coordinator, RoundOutcome,
    RoundSpec,
};
use horch_core::competition::judging::DetachedLauncher;
use horch_core::competition::model::RoundState;
use horch_core::competition::observe::{self, TelemetryUsage};
use horch_core::competition::planner::{plan_round, PlanInput, RoundPlan};
use horch_core::competition::preflight::{
    evaluate, recorded_estimates, PreflightCandidate, PreflightPlan, PreflightReport, TokenEstimate,
};
use horch_core::competition::report::{self, RoundReport};
use horch_core::evaluation::validator::{CommandValidator, GateSpec};
use horch_core::execution::store::ExecutionStore;
use horch_core::fsx;
use horch_core::ids::{ExperimentId, RoundId, TeammateName};
use horch_core::measure::event::EventEnvelope;
use horch_core::measure::paths::{component, DatasetPaths};
use horch_core::measure::projection::{fold, ExperimentView, Projection, RoundView};
use horch_core::measure::recorder::JsonlRecorder;
use horch_core::measure::redact::redact;
use horch_core::measure::store::{self, StoreOptions};
use horch_core::roster::Roster;
use horch_core::routing::eligible::EligibilityFilter;
use horch_core::routing::policy::Policy;
use horch_core::routing::quota::QuotaView;
use horch_core::routing::snapshot::{obtain, QuotaEnv};
use horch_core::runtime::machine::{self, Known};
use horch_core::runtime::RuntimeContext;
use horch_core::usage::Locations;
use horch_core::vcs::git::GitCli;
use horch_core::vcs::worktree::WorktreeSpec;
use horch_core::workspace::herdr::Herdr;
use horch_core::workspace::model::Direction;
use horch_core::workspace::paneshell::PaneShell;
use serde::{Deserialize, Serialize};

use super::cli::{plan_task, RunArgs};
use super::preflight::{self, ExperimentFacts, GatherInput};
use super::{abort_on_fault, dataset_paths, exit};
use crate::project::project_facts;

pub(crate) fn run(
    ctx: &mut RuntimeContext,
    env: &BTreeMap<String, String>,
    args: &RunArgs,
) -> Result<u8> {
    let report_to = match report_target(ctx, args) {
        Ok(report_to) => report_to,
        Err(usage) => {
            eprintln!("multi-herdr-dataset: {usage}");
            return Ok(exit::USAGE);
        }
    };
    let paths = dataset_paths(ctx)?;
    let project = ctx.paths.project()?;
    let plan = args
        .plan
        .as_deref()
        .map(|p| plan_in_project(&project, ctx.paths.cwd.as_deref(), p));
    let task = task_of(args, plan.as_deref())?;
    let experiment = ExperimentId::mint(clock::now());
    let saved = SavedRun::from_args(args, &task, plan, report_to);
    saved.write(&paths, &experiment)?;
    let start = if args.detach {
        Start::Detached
    } else {
        Start::Here
    };
    preflight_and_run(ctx, env, &paths, &experiment, &task, &saved, start)
}

/// The pane that gets the report line. `--detach` reports to the caller's
/// pane by default; with no pane to report to, nothing is created (FDS-10):
/// that gives the usage error.
fn report_target(ctx: &RuntimeContext, args: &RunArgs) -> Result<Option<String>, &'static str> {
    match (&args.report_to, args.detach, &ctx.herdr.pane) {
        (Some(pane), _, _) => Ok(Some(pane.clone())),
        (None, true, Some(internal)) => Ok(Some(public_pane(ctx, internal.as_str()))),
        (None, true, None) => Err("run --detach needs a pane to report to: \
                                   pass --report-to <PANE>, or run it in a herdr pane (HERDR_PANE_ID)"),
        (None, false, _) => Ok(None),
    }
}

/// The task of the run: the `TASK` argument, else the plan's task.
fn task_of(args: &RunArgs, plan: Option<&str>) -> Result<String> {
    match (&args.task, plan) {
        (Some(task), _) => Ok(task.clone()),
        (None, Some(plan)) => Ok(plan_task(plan)),
        (None, None) => bail!("run needs a TASK or --plan"),
    }
}

/// Where a round that passed preflight runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Start {
    /// The coordinator runs in this process.
    Here,
    /// `--detach`: the coordinator runs in a new pane of the round's
    /// dataset workspace, and this process returns (FDS-10).
    Detached,
}

/// The public id of the pane with herdr's internal id `internal`
/// (`HERDR_PANE_ID`), as `horch tell` names panes; the internal id when
/// herdr does not answer.
fn public_pane(ctx: &RuntimeContext, internal: &str) -> String {
    Herdr::with_bin(&ctx.bins.harness.herdr)
        .pane_get(internal)
        .map(|p| p.pane_id)
        .unwrap_or_else(|_| internal.to_string())
}

/// `--plan` as a path relative to the project root, `/`-separated (FDS-09).
/// A relative `plan` is relative to `cwd`. A path outside the project stays
/// as given; preflight then refuses it as untracked.
fn plan_in_project(project: &Path, cwd: Option<&Path>, plan: &Path) -> String {
    let full = match cwd {
        Some(cwd) if plan.is_relative() => cwd.join(plan),
        _ => plan.to_path_buf(),
    };
    let rel = full
        .strip_prefix(project)
        .ok()
        .map(Path::to_path_buf)
        .or_else(|| {
            let (full, project) = (full.canonicalize().ok()?, project.canonicalize().ok()?);
            full.strip_prefix(project).ok().map(Path::to_path_buf)
        });
    let Some(rel) = rel else {
        return plan.to_string_lossy().into_owned();
    };
    rel.components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// `resume <exp>` with the report target the experiment saved.
pub(crate) fn resume(
    ctx: &mut RuntimeContext,
    env: &BTreeMap<String, String>,
    exp: &str,
) -> Result<u8> {
    resume_reporting(ctx, env, exp, None)
}

/// `resume <exp>`: re-enter the experiment where its events stop. Preflight
/// runs again when it never completed; a planned round is planned again with
/// its own id; a running round is driven on, adopting every candidate that a
/// killed coordinator started.
///
/// `report_to` (`--report-to`) replaces the saved report target.
pub(crate) fn resume_reporting(
    ctx: &mut RuntimeContext,
    env: &BTreeMap<String, String>,
    exp: &str,
    report_to: Option<&str>,
) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let experiment = ExperimentId::new(exp)?;
    let projection = fold(&store::read_all(&paths)?.events);
    let saved = saved_for_resume(&paths, &experiment, report_to)?;
    match resume_plan(&projection, &experiment)? {
        Resume::Fresh => preflight_and_run(
            ctx,
            env,
            &paths,
            &experiment,
            &saved.task,
            &saved,
            Start::Here,
        ),
        Resume::Aborted => {
            println!("experiment {experiment} was aborted by preflight");
            Ok(exit::PREFLIGHT_FAILED)
        }
        Resume::Continue(x) => continue_round(ctx, &paths, &experiment, &saved, &projection, x),
    }
}

/// The run the experiment saved, with `report_to` (`--report-to`) as its
/// report target when given.
fn saved_for_resume(
    paths: &DatasetPaths,
    experiment: &ExperimentId,
    report_to: Option<&str>,
) -> Result<SavedRun> {
    let mut saved = SavedRun::read(paths, experiment)?;
    if let Some(pane) = report_to {
        saved.report_to = Some(pane.to_string());
    }
    Ok(saved)
}

/// Where `resume` re-enters an experiment.
#[derive(Debug)]
enum Resume<'a> {
    /// Preflight never completed: it runs again under the same id.
    Fresh,
    /// Preflight aborted the experiment: nothing to resume.
    Aborted,
    /// Preflight passed: its round goes on.
    Continue(&'a ExperimentView),
}

/// Where the events of `experiment` stop.
fn resume_plan<'a>(projection: &'a Projection, experiment: &ExperimentId) -> Result<Resume<'a>> {
    // Killed before `experiment.created`: start over under the same id.
    let Some(x) = projection.experiments.get(experiment) else {
        return Ok(Resume::Fresh);
    };
    match x.state {
        RoundState::Aborted => Ok(Resume::Aborted),
        RoundState::Preflight if x.preflight.is_none() => Ok(Resume::Fresh),
        RoundState::Preflight => {
            bail!("experiment {experiment} failed preflight but was not aborted")
        }
        _ => Ok(Resume::Continue(x)),
    }
}

/// Run the round of `x` again from where its events stop.
fn continue_round(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    experiment: &ExperimentId,
    saved: &SavedRun,
    projection: &Projection,
    x: &ExperimentView,
) -> Result<u8> {
    let report = x
        .preflight
        .clone()
        .context("the experiment has no preflight report")?;
    let config = manifest_config(paths, experiment)?;
    let ResumedRound {
        round,
        plan,
        mut planned_models,
    } = round_and_plan(ctx, &config, projection, x)?;
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
        paths,
        &Round {
            experiment,
            round: &round,
            config: &config,
            task: &saved.task,
            safe_n: report.safe_n,
            estimates,
            report_to: saved.report_to.as_deref(),
        },
        plan.as_ref(),
    )
}

/// The round that `resume` drives, its plan when it is still planned or
/// new, and the model planned for each label of the round so far.
struct ResumedRound {
    round: RoundId,
    plan: Option<RoundPlan>,
    planned_models: Vec<(String, String)>,
}

/// The round that `resume` drives: the latest round of `x`, or a new one.
fn round_and_plan(
    ctx: &RuntimeContext,
    config: &DatasetConfig,
    projection: &Projection,
    x: &ExperimentView,
) -> Result<ResumedRound> {
    let base_sha = &x.created.base_sha;
    let Some(round) = x.rounds.last() else {
        let round = RoundId::mint(clock::now());
        let plan = plan_for(ctx, config, &round, base_sha)?;
        return Ok(ResumedRound {
            round,
            plan: Some(plan),
            planned_models: Vec::new(),
        });
    };
    let view = &projection.rounds[round];
    let plan = (view.state == RoundState::Planned)
        .then(|| plan_for(ctx, config, round, base_sha))
        .transpose()?;
    let models = view
        .candidates
        .iter()
        .filter_map(|(l, c)| Some((l.clone(), c.planned.as_ref()?.model.to_string())))
        .collect();
    Ok(ResumedRound {
        round: round.clone(),
        plan,
        planned_models: models,
    })
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
    start: Start,
) -> Result<u8> {
    let now = clock::now();
    let round_id = RoundId::mint(now);
    let gathered = gather_report(ctx, paths, experiment, task, saved, &round_id)?;

    let recorder = JsonlRecorder::open(paths, StoreOptions::from_faults(&ctx.settings.faults))?;
    let failed = preflight::record(
        &recorder,
        paths,
        &ExperimentFacts {
            experiment,
            task,
            plan: &gathered.pre_plan,
            report: &gathered.report,
            env: snapshot_env(ctx, env),
            at: now,
        },
        &ctx.settings.faults,
    )?;
    drop(recorder);

    // A detached pass prints its 1 line only (FDS-10); a refusal prints all.
    if start == Start::Here || !failed.is_empty() {
        println!("experiment {experiment}");
        print!("{}", render_plan(&gathered.plan));
        print!("{}", preflight::render(&gathered.report));
    }
    if let Some((line, code)) = refusal(gathered.plan.candidates.is_empty(), &failed) {
        println!("{line}");
        Ok(code)
    } else {
        let attempt = Attempt {
            experiment,
            round: &round_id,
            task,
            saved,
            start,
        };
        launch(ctx, paths, &attempt, &gathered)
    }
}

/// What preflight looked at and found.
struct Gathered {
    config: DatasetConfig,
    plan: RoundPlan,
    pre_plan: PreflightPlan,
    report: PreflightReport,
}

/// Plan the round and run the preflight checks on the plan.
fn gather_report(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    experiment: &ExperimentId,
    task: &str,
    saved: &SavedRun,
    round_id: &RoundId,
) -> Result<Gathered> {
    let project = ctx.paths.project()?;
    let flags = saved.flags()?;
    let mut config = config::load(&project, &flags)?;

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
    let (plan, view) = plan_with_view(ctx, &config, round_id, head.as_deref().unwrap_or(""))?;

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
    let mut report = evaluate(&pre_plan, &snapshot);
    let problem = saved
        .plan
        .as_deref()
        .and_then(|plan| plan_file_problem(&git, &project, &pre_plan, plan));
    if let Some(problem) = problem {
        preflight::fail_pre_01(&mut report, &problem);
    }
    Ok(Gathered {
        config,
        plan,
        pre_plan,
        report,
    })
}

/// Why the `--plan` file cannot go with the round, if it cannot.
fn plan_file_problem(
    git: &GitCli,
    project: &Path,
    pre_plan: &PreflightPlan,
    plan: &str,
) -> Option<String> {
    let dir = pre_plan.git.toplevel.as_deref().unwrap_or(project);
    preflight::plan_problem(git, dir, pre_plan.git.base_sha.as_deref(), plan)
}

/// The line and exit code that refuse a round after preflight, or `None`
/// when the round may start. An empty plan fails PRE-08, but the planner
/// refused every candidate (excluded, or over its usage limits): that is
/// the budget/quota refusal, exit 3 (dataset design §6 B2), not a failed
/// machine check. The report is recorded and the experiment aborted all the
/// same.
fn refusal(no_candidates: bool, failed: &[String]) -> Option<(String, u8)> {
    const NOTHING_STARTED: &str = "No worktree was created and no model was called.";
    if no_candidates && failed.iter().all(|id| id == "PRE-08") {
        Some((
            format!("REFUSED: no candidate can run within the usage limits. {NOTHING_STARTED}"),
            exit::BUDGET_REFUSED,
        ))
    } else if !failed.is_empty() {
        Some((
            format!(
                "REFUSED: preflight failed ({}). {NOTHING_STARTED}",
                failed.join(", ")
            ),
            exit::PREFLIGHT_FAILED,
        ))
    } else {
        None
    }
}

/// One attempt of an experiment's first round.
struct Attempt<'a> {
    experiment: &'a ExperimentId,
    round: &'a RoundId,
    task: &'a str,
    saved: &'a SavedRun,
    start: Start,
}

/// Start the round that passed preflight, in a new pane or here.
fn launch(
    ctx: &mut RuntimeContext,
    paths: &DatasetPaths,
    a: &Attempt<'_>,
    g: &Gathered,
) -> Result<u8> {
    ctx.settings.faults.abort_if("abort-after-preflight");
    if a.start == Start::Detached {
        return start_detached(ctx, paths, a.experiment, a.round, &g.plan, a.saved);
    }
    let pre_plan = &g.pre_plan;
    coordinate(
        ctx,
        paths,
        &Round {
            experiment: a.experiment,
            round: a.round,
            config: &g.config,
            task: a.task,
            safe_n: g.report.safe_n,
            estimates: meter_estimates(
                pre_plan
                    .candidates
                    .iter()
                    .map(|c| (c.label.as_str(), c.model.as_str())),
                &pre_plan.expected_tokens,
                &g.config.budget.expected_tokens,
                &pre_plan.measured_tokens,
            ),
            report_to: a.saved.report_to.as_deref(),
        },
        Some(&g.plan),
    )
}

/// `--detach` after a passed preflight: record the round's plan, open its
/// dataset workspace, and run `resume <experiment> --report-to <pane>` in a
/// new pane there. The resumed coordinator drives the round from its plan
/// and reports at the end. The pane is a visible herdr pane, not a
/// background job (CMP-16).
fn start_detached(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    experiment: &ExperimentId,
    round: &RoundId,
    plan: &RoundPlan,
    saved: &SavedRun,
) -> Result<u8> {
    let project = ctx.paths.project()?;
    let report_to = saved
        .report_to
        .as_deref()
        .context("a detached round has no pane to report to")?;
    record_plan(ctx, paths, experiment, round, plan)?;
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let watch = watch_command(ctx, &project, experiment)?;
    let ws = dataset_workspace(&herdr, paths, experiment, &project, &watch)?;
    let pane = herdr
        .pane_split(&ws.root_pane, Direction::Down)
        .context("opening the coordinator pane in the dataset workspace")?;
    let command = resume_command(ctx, &project, experiment, report_to)?;
    if let Err(e) = herdr.pane_run(&pane, &command) {
        let _ = herdr.pane_close(&pane);
        return Err(e.context(format!(
            "starting the coordinator of round {round}; run multi-herdr-dataset resume {experiment}"
        )));
    }
    println!(
        "round {round} started: workspace {}, report to {report_to}",
        workspace_name(experiment)
    );
    Ok(exit::SUCCESS)
}

/// Record the round's plan before the detached coordinator resumes it.
fn record_plan(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    experiment: &ExperimentId,
    round: &RoundId,
    plan: &RoundPlan,
) -> Result<()> {
    let recorder = JsonlRecorder::open(paths, StoreOptions::from_faults(&ctx.settings.faults))?;
    record_round_plan(&recorder, experiment, round, plan, &monotonic(clock::now))
}

/// The shell command that runs `resume <experiment> --report-to <pane>` in
/// the new pane, with the settings a pane does not inherit.
fn resume_command(
    ctx: &RuntimeContext,
    project: &Path,
    experiment: &ExperimentId,
    report_to: &str,
) -> Result<String> {
    let exe = ctx.bins.exe()?;
    let env = pane_env(ctx);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    Ok(PaneShell::host().command_line_with_env(
        &exe,
        &env,
        &[
            "resume".to_string(),
            experiment.to_string(),
            "--report-to".to_string(),
            report_to.to_string(),
            "--project".to_string(),
            project.to_string_lossy().into_owned(),
        ],
    ))
}

/// What the dataset workspace's root pane runs: `watch <experiment>`.
fn watch_command(
    ctx: &RuntimeContext,
    project: &Path,
    experiment: &ExperimentId,
) -> Result<String> {
    let exe = ctx.bins.exe()?;
    let data_root = ctx.paths.data_root.to_string_lossy();
    Ok(PaneShell::host().command_line_with_env(
        &exe,
        &[("HORCH_DATA_DIR", data_root.as_ref())],
        &[
            "watch".to_string(),
            experiment.to_string(),
            "--state-dir".to_string(),
            ctx.paths.state_root.to_string_lossy().into_owned(),
            "--project".to_string(),
            project.to_string_lossy().into_owned(),
        ],
    ))
}

/// The settings a detached coordinator needs from this process: a pane
/// does not inherit its environment. The stores, the roster, the binaries,
/// and the machine, quota and balance overrides.
fn pane_env(ctx: &RuntimeContext) -> Vec<(&'static str, String)> {
    let text = |p: &Path| p.to_string_lossy().into_owned();
    let mut out = vec![
        ("HORCH_DATA_DIR", text(&ctx.paths.data_root)),
        ("HORCH_STATE_DIR", text(&ctx.paths.state_root)),
    ];
    if let Some(dir) = &ctx.bins.roster_override {
        out.push(("HORCH_TEAMMATES_DIR", text(dir)));
    }
    if let Some(balance) = &ctx.settings.balance_override {
        out.push(("HORCH_BALANCE", balance.clone()));
    }
    if let Some(file) = &ctx.settings.quota_file {
        out.push(("HORCH_QUOTA_FILE", text(file)));
    }
    if let Some(file) = &ctx.settings.machine_file {
        out.push(("HORCH_MACHINE_FILE", text(file)));
    }
    for (key, value) in ctx.bins.overrides.env_pairs() {
        out.push((key, text(&value)));
    }
    out
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
    /// The pane that gets the round's report line (FDS-11).
    report_to: Option<&'a str>,
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
    let validator = round_validator(ctx, paths, r)?;
    let usage = TelemetryUsage {
        locations: Locations::from_context(ctx),
    };
    let meter = UsageMeter {
        estimates: r.estimates.clone(),
        ..UsageMeter::default()
    };
    let spec = round_spec(ctx, r, project)?;
    let disk_free = disk_probe(ctx, &spec.worktree_root);
    let clock = monotonic(clock::now);
    let sleep = |d: Duration| std::thread::sleep(d);
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
    drive_round(&coordinator, &spec, r, &herdr, plan)
}

/// Start the planned round, or drive the one that has events. A coordinator
/// that stops on an error reports it; one that ends the round finishes it.
fn drive_round(
    coordinator: &Coordinator<'_, GitCli>,
    spec: &RoundSpec,
    r: &Round,
    herdr: &Herdr,
    plan: Option<&RoundPlan>,
) -> Result<u8> {
    let outcome = match plan {
        Some(plan) => coordinator.start(spec, plan),
        None => coordinator.drive(spec),
    };
    match outcome {
        Ok(outcome) => finish_round(coordinator, spec, r, herdr, &outcome),
        Err(e) => Err(report_stopped(herdr, r, e)),
    }
}

/// The round the coordinator runs, in `project`.
fn round_spec(ctx: &RuntimeContext, r: &Round, project: PathBuf) -> Result<RoundSpec> {
    let worktree_root = r
        .config
        .worktree_root
        .clone()
        .context("the config has no worktree root")?;
    Ok(RoundSpec {
        experiment: r.experiment.clone(),
        round: r.round.clone(),
        index: 0,
        config: r.config.clone(),
        task: r.task.to_string(),
        watch_command: watch_command(ctx, &project, r.experiment)?,
        repo: project,
        worktree_root,
        safe_n: r.safe_n,
        tick: TICK,
    })
}

/// The validator of the round: the config's gates, logging under the
/// round's artifacts. `fail-gate:<name>` reaches the gates from `run` (and
/// from promotion's revalidation, which uses this validator too).
fn round_validator(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    r: &Round,
) -> Result<CommandValidator> {
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
    Ok(CommandValidator::new(
        gates,
        artifacts,
        ctx.settings.faults.points().clone(),
    ))
}

/// Free bytes on the disk that the worktree root lands on; `None` when the
/// probe does not know.
fn disk_probe(ctx: &RuntimeContext, worktree_root: &Path) -> impl Fn() -> Option<u64> {
    let disk_dir = existing_ancestor(worktree_root);
    let machine_file = ctx.settings.machine_file.clone();
    move || {
        let probe = machine::probe(
            &disk_dir,
            &machine::ProbeBins::default(),
            machine_file.as_deref(),
        );
        match probe.disk_free_bytes {
            Known::Known(free) => Some(free),
            Known::Unknown => None,
        }
    }
}

/// The coordinator stopped on an error: abort the process on a fault point,
/// else report to the pane that waits. Gives the error back.
fn report_stopped(herdr: &Herdr, r: &Round, e: anyhow::Error) -> anyhow::Error {
    abort_on_fault(&e);
    if let Some(pane) = r.report_to {
        let stopped = RoundReport::stopped(r.experiment, r.round, &format!("{e:#}"));
        report::deliver(herdr, pane, &stopped);
    }
    e
}

/// The round ended: report it, close its workspace, print its line. Gives
/// the exit code.
fn finish_round(
    coordinator: &Coordinator<'_, GitCli>,
    spec: &RoundSpec,
    r: &Round,
    herdr: &Herdr,
    outcome: &RoundOutcome,
) -> Result<u8> {
    let view = coordinator.view(spec)?;
    let events = store::read_all(coordinator.paths)?.events;
    let reason = latest_intervention(&events, r.round);
    // The report goes before the workspace closes: a detached coordinator
    // runs in a pane of that workspace.
    if let Some(pane) = r.report_to {
        let report =
            RoundReport::from_view(r.experiment, r.round, outcome, &view, reason.as_deref());
        report::deliver(herdr, pane, &report);
    }
    coordinator.close_workspace(spec);
    let (line, code) = outcome_line(outcome, &view, reason.as_deref());
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
        RoundOutcome::Decided => (decided_line(view), exit::SUCCESS),
        RoundOutcome::Rejected { budget: true } => (
            "REJECTED: the budget stopped the candidates".to_string(),
            exit::BUDGET_REFUSED,
        ),
        RoundOutcome::Rejected { budget: false } => {
            (with_reason("REJECTED", view.rejected), exit::REJECTED)
        }
        RoundOutcome::NeedsIntervention => (
            with_reason(
                "NEEDS_INTERVENTION",
                reason.or(view.needs_intervention.as_ref().map(|n| n.reason.as_str())),
            ),
            exit::NEEDS_INTERVENTION,
        ),
    }
}

/// The line of a decided round: where the promotion put the winner, else the
/// winner, else no more than the state.
fn decided_line(view: &RoundView) -> String {
    let p = &view.promotion;
    match (&p.started, &p.completed, &view.winner) {
        (Some(started), Some(done), _) => {
            format!("PROMOTED: {} is at {}", started.target, done.dest_after)
        }
        (_, _, Some(w)) => format!("DECIDED: winner {}", w.label),
        _ => "DECIDED".to_string(),
    }
}

/// `head: reason`, or `head` when there is no reason.
fn with_reason(head: &str, reason: Option<impl std::fmt::Display>) -> String {
    match reason {
        Some(reason) => format!("{head}: {reason}"),
        None => head.to_string(),
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

/// The roster with the project's facts attached, as `horch spawn` and the
/// fleet load it: a candidate's `skills_when` skills come from the
/// repository the round runs in (U-26).
fn load_roster(ctx: &RuntimeContext) -> Result<Roster> {
    let roster = Roster::load_layered(
        ctx.inherited.home_var.as_deref().map(Path::new),
        ctx.bins.roster_override.as_deref(),
        None,
    )?;
    warn_once(&roster, &ROSTER_WARNED, &mut std::io::stderr());
    Ok(roster.with_project_facts(project_facts(&ctx.paths.project()?)))
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
    /// The `--plan` file, relative to the project root: preflight checks it
    /// again on a resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// The pane that gets the round's report line (`--report-to`, or the
    /// caller's pane with `--detach`). `resume --report-to` replaces it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_to: Option<String>,
}

impl SavedRun {
    fn from_args(
        args: &RunArgs,
        task: &str,
        plan: Option<String>,
        report_to: Option<String>,
    ) -> Self {
        SavedRun {
            task: redact(task).into_owned(),
            candidates: args.candidates,
            strategy: args.strategy,
            budget_usd: args.budget_usd.clone(),
            judge: args.judge,
            baseline: args.baseline.clone(),
            promote_to: args.promote_to.clone(),
            worktree_root: args.worktree_root.clone(),
            allow_dirty: args.allow_dirty,
            plan,
            report_to,
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
    use crate::dataset::test_fixtures::{exp, experiment, projection, round_view};

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

    /// U-26: a candidate launches with the `skills_when` skills of the
    /// project the dataset run targets, as a `horch spawn` worker does. A
    /// Godot C# project (`*.csproj`) adds the C# skills.
    #[test]
    fn dataset_roster_carries_the_project_facts() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("game");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("project.godot"), "").unwrap();
        std::fs::write(project.join("Game.csproj"), "").unwrap();
        let teammates = tmp.path().join("teammates");
        std::fs::create_dir_all(&teammates).unwrap();
        std::fs::write(
            teammates.join("csharp-candidate.md"),
            "---\nname: csharp-candidate\nbrief_description: X\nagent: claude\n\
             model: opus\nskills: [tdd]\n\
             skills_when: {\"*.csproj\": [godot-csharp-godot]}\n---\nbody",
        )
        .unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new(&project)
                .with("HOME", home.to_str().unwrap())
                .with("HORCH_TEAMMATES_DIR", teammates.to_str().unwrap())
                .with_exe("/opt/horch/bin/horch"),
        )
        .unwrap();
        let roster = load_roster(&ctx).unwrap();
        let t = roster.require("csharp-candidate").unwrap().clone();
        let launched = roster.for_launch(t);
        assert!(
            launched.skills.iter().any(|s| s == "godot-csharp-godot"),
            "{:?}",
            launched.skills
        );
    }

    fn decided() -> RoundView {
        round_view(RoundState::Complete, Vec::new())
    }

    #[test]
    fn outcome_line_for_a_decided_round_names_the_winner_or_the_promotion() {
        let mut view = decided();
        assert_eq!(
            outcome_line(&RoundOutcome::Decided, &view, None),
            ("DECIDED".to_string(), 0)
        );
        view.winner = Some(
            serde_json::from_value(serde_json::json!({
                "label": "B",
                "execution_id": "0199a5b0-0000-7000-8000-000000000c01",
                "head_sha": "h",
                "judgment_id": "0199a5b0-0000-7000-8000-000000000d01",
                "promotion": "not_requested",
            }))
            .unwrap(),
        );
        assert_eq!(
            outcome_line(&RoundOutcome::Decided, &view, None),
            ("DECIDED: winner B".to_string(), 0)
        );
        view.promotion = serde_json::from_value(serde_json::json!({
            "started": {"target": "main", "dest_before": "a", "planned_after": "b",
                        "strategy": "fast_forward"},
            "completed": {"receipt_digest": horch_core::measure::digest::sha256_bytes(b"r"),
                          "dest_after": "bbb"},
            "conflicted": null,
            "rolled_back": [],
        }))
        .unwrap();
        assert_eq!(
            outcome_line(&RoundOutcome::Decided, &view, None),
            ("PROMOTED: main is at bbb".to_string(), 0)
        );
    }

    #[test]
    fn outcome_line_for_a_rejected_round() {
        use horch_core::evaluation::winner::RejectReason;
        let mut view = decided();
        assert_eq!(
            outcome_line(&RoundOutcome::Rejected { budget: true }, &view, None),
            ("REJECTED: the budget stopped the candidates".to_string(), 3)
        );
        assert_eq!(
            outcome_line(&RoundOutcome::Rejected { budget: false }, &view, None),
            ("REJECTED".to_string(), 6)
        );
        view.rejected = Some(RejectReason::BelowConfidence);
        assert_eq!(
            outcome_line(&RoundOutcome::Rejected { budget: false }, &view, None),
            ("REJECTED: below_confidence".to_string(), 6)
        );
    }

    #[test]
    fn outcome_line_for_a_round_that_needs_the_operator_prefers_the_latest_reason() {
        let mut view = decided();
        assert_eq!(
            outcome_line(&RoundOutcome::NeedsIntervention, &view, None),
            ("NEEDS_INTERVENTION".to_string(), 5)
        );
        view.needs_intervention = serde_json::from_value(serde_json::json!(
            {"reason": "first", "source": "judge"}
        ))
        .ok();
        assert_eq!(
            outcome_line(&RoundOutcome::NeedsIntervention, &view, None),
            ("NEEDS_INTERVENTION: first".to_string(), 5)
        );
        assert_eq!(
            outcome_line(&RoundOutcome::NeedsIntervention, &view, Some("latest")),
            ("NEEDS_INTERVENTION: latest".to_string(), 5)
        );
    }

    #[test]
    fn resume_starts_over_for_an_experiment_with_no_events() {
        let p = Projection::default();
        assert!(matches!(resume_plan(&p, &exp()).unwrap(), Resume::Fresh));
    }

    #[test]
    fn resume_does_not_restart_an_aborted_experiment() {
        let p = projection(RoundState::Aborted, &[]);
        assert!(matches!(resume_plan(&p, &exp()).unwrap(), Resume::Aborted));
    }

    #[test]
    fn resume_runs_preflight_again_when_it_never_completed() {
        let p = projection(RoundState::Preflight, &[]);
        assert!(matches!(resume_plan(&p, &exp()).unwrap(), Resume::Fresh));
    }

    #[test]
    fn resume_refuses_a_failed_preflight_that_was_not_aborted() {
        let mut p = projection(RoundState::Preflight, &[]);
        p.experiments.get_mut(&exp()).unwrap().preflight =
            serde_json::from_value(serde_json::json!({
                "schema_version": "1.0.0", "checks": [], "safe_n": 1, "waves": 1,
                "projected_cost_microusd": 0,
                "machine": {"os": "macos", "arch": "aarch64", "cpus": 10,
                            "mem_total_bytes": null, "mem_available_bytes": null,
                            "disk_free_bytes": null, "disk_total_bytes": null,
                            "gpu": "apple_silicon", "max_open_files": null,
                            "max_processes": null},
                "environment_digest": horch_core::measure::digest::sha256_bytes(b"e"),
                "passed": false,
            }))
            .unwrap();
        assert_eq!(
            resume_plan(&p, &exp()).unwrap_err().to_string(),
            format!("experiment {} failed preflight but was not aborted", exp())
        );
    }

    #[test]
    fn resume_continues_a_planned_experiment() {
        let p = projection(RoundState::Planned, &[RoundState::Running]);
        let Resume::Continue(x) = resume_plan(&p, &exp()).unwrap() else {
            panic!("not continue");
        };
        assert_eq!(x, &experiment(RoundState::Planned, x.rounds.clone()));
    }

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn refusal_is_none_for_a_plan_that_passed() {
        assert_eq!(refusal(false, &[]), None);
    }

    #[test]
    fn refusal_is_the_budget_refusal_when_only_the_empty_plan_failed() {
        assert_eq!(
            refusal(true, &ids(&["PRE-08"])),
            Some((
                "REFUSED: no candidate can run within the usage limits. \
                 No worktree was created and no model was called."
                    .to_string(),
                3
            ))
        );
    }

    #[test]
    fn refusal_names_every_failed_check_of_a_failed_preflight() {
        let expected = Some((
            "REFUSED: preflight failed (PRE-01, PRE-08). \
             No worktree was created and no model was called."
                .to_string(),
            4,
        ));
        assert_eq!(refusal(true, &ids(&["PRE-01", "PRE-08"])), expected);
        assert_eq!(refusal(false, &ids(&["PRE-01", "PRE-08"])), expected);
    }

    fn run_args(argv: &[&str]) -> RunArgs {
        use clap::Parser;
        let cli = crate::dataset::cli::Cli::try_parse_from(
            ["multi-herdr-dataset", "run"]
                .into_iter()
                .chain(argv.iter().copied()),
        )
        .unwrap();
        let crate::dataset::cli::Command::Run(args) = cli.command else {
            panic!("not run");
        };
        args
    }

    #[test]
    fn the_task_comes_from_the_argument_before_the_plan() {
        let args = run_args(&["fix the bug"]);
        assert_eq!(task_of(&args, Some("plan.md")).unwrap(), "fix the bug");
    }

    #[test]
    fn the_task_comes_from_the_plan_without_an_argument() {
        let args = run_args(&["--plan", "plan.md"]);
        assert_eq!(
            task_of(&args, Some("plan.md")).unwrap(),
            "Read and follow plan.md exactly."
        );
    }

    #[test]
    fn a_run_with_no_task_and_no_plan_is_refused() {
        // clap requires one of them; the plan can still be missing after
        // `plan_in_project`.
        let args = run_args(&["--plan", "plan.md"]);
        assert_eq!(
            task_of(&args, None).unwrap_err().to_string(),
            "run needs a TASK or --plan"
        );
    }
}
