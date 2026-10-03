//! `multi-herdr-dataset run`: load the config, plan the round, run
//! preflight, and refuse (exit 4) before any worktree or model call.
//!
//! B3: the coordinator replaces the end of [`run`]. Today a passed preflight
//! prints the plan and stops: rounds are not implemented in this build.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use horch_core::clock;
use horch_core::competition::config::{self, DatasetConfig};
use horch_core::competition::planner::{plan_round, PlanInput, RoundPlan};
use horch_core::competition::preflight::{evaluate, PreflightCandidate};
use horch_core::ids::{ExperimentId, RoundId};
use horch_core::measure::recorder::JsonlRecorder;
use horch_core::measure::store::StoreOptions;
use horch_core::routing::eligible::EligibilityFilter;
use horch_core::routing::policy::Policy;
use horch_core::routing::snapshot::{obtain, QuotaEnv};
use horch_core::runtime::RuntimeContext;
use horch_core::teammates::Roster;
use horch_core::vcs::git::GitCli;
use horch_core::vcs::worktree::WorktreeSpec;

use super::cli::RunArgs;
use super::preflight::{self, ExperimentFacts, GatherInput};
use super::{dataset_paths, exit};

pub fn run(ctx: &mut RuntimeContext, env: &BTreeMap<String, String>, args: &RunArgs) -> Result<u8> {
    let project = ctx.paths.project()?;
    let mut config = config::load(&project, &args.flags()?)?;
    let paths = dataset_paths(ctx)?;
    let now = clock::now();
    let experiment = ExperimentId::mint(now);
    let round_id = RoundId::mint(now);

    // PRE-13 checks the root against the trusted directories, so it must be
    // absolute. A relative root is relative to the project.
    let root = match config.worktree_root.clone() {
        Some(root) => root,
        None => paths.default_worktree_root(&experiment)?,
    };
    config.worktree_root = Some(if root.is_absolute() {
        root
    } else {
        project.join(root)
    });

    let git = GitCli::new(ctx.bins.harness.git.clone());
    let head = preflight::repo_facts(&git, &project, &[]).base_sha;

    let roster = Roster::load_layered(
        ctx.inherited.home_var.as_deref().map(Path::new),
        ctx.bins.roster_override.as_deref(),
        None,
    )?;
    let policy = Policy::load(
        &ctx.paths.state_root,
        ctx.settings.balance_override.as_deref(),
    )?;
    // No probe: preflight makes no call that could cost a model turn.
    let view = obtain(
        &ctx.paths.state_root,
        now,
        &policy,
        false,
        &QuotaEnv::from_context(ctx),
    )?;
    let plan = plan_round(&PlanInput {
        round_id: &round_id,
        index: 0,
        base_sha: head.as_deref().unwrap_or(""),
        n: config.candidates,
        baseline: args.flags()?.baseline,
        roster: &roster,
        view: &view,
        filter: &EligibilityFilter::default(),
        config: &config,
    });

    let branches = planned_branches(&experiment, &plan, &config);
    let git_facts = preflight::repo_facts(&git, &project, &branches);
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
            paths: &paths,
            config: &config,
            candidates,
            git: git_facts,
            view: &view,
        },
    );
    let report = evaluate(&pre_plan, &snapshot);

    let recorder = JsonlRecorder::open(&paths, StoreOptions::from_faults(&ctx.settings.faults))?;
    let failed = preflight::record(
        &recorder,
        &paths,
        &ExperimentFacts {
            experiment: &experiment,
            task: &args.task,
            plan: &pre_plan,
            report: &report,
            env: snapshot_env(ctx, env),
            at: now,
        },
    )?;

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
    // B3: the coordinator starts the round here.
    println!("preflight passed. Rounds are not implemented in this build.");
    Ok(exit::SUCCESS)
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
                exp8: exp8(experiment),
                round_index: plan.index,
                label: c.label.to_string(),
                base_sha: plan.base_sha.clone(),
            }
            .branch()
        })
        .collect()
}

/// The first 8 characters of the experiment id.
pub fn exp8(experiment: &ExperimentId) -> String {
    experiment.as_str().chars().take(8).collect()
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
