//! Preflight end to end (B2): gather the real facts, evaluate them with the
//! pure `competition::preflight::evaluate`, and record the report.
//!
//! Every fact here is read before any worktree exists and without a model
//! turn: harnesses answer `--version` only (PRE-06, PRE-07). What is
//! persisted passes through `redact` and `envsnap` first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use horch_core::competition::config::DatasetConfig;
use horch_core::competition::preflight::{
    storage_probe, CheckStatus, GitFacts, PoolFacts, PreflightCandidate, PreflightPlan,
    PreflightReport, StorageProbe,
};
use horch_core::fsx;
use horch_core::harness::HarnessKind;
use horch_core::herdr::Herdr;
use horch_core::ids::{ExperimentId, TaskId};
use horch_core::measure::digest::{digest_json, sha256_bytes, Digest};
use horch_core::measure::envsnap::env_snapshot;
use horch_core::measure::event::{
    Actor, EventKind, ExperimentAborted, ExperimentCreated, PreflightCompleted,
};
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::recorder::{NewEvent, Recorder};
use horch_core::measure::redact::redact;
use horch_core::routing::quota::{pool_for, QuotaView};
use horch_core::routing::quota_probe::harness_version;
use horch_core::runtime::machine::{self, MachineSnapshot};
use horch_core::runtime::RuntimeContext;
use horch_core::vcs::git::{GitCli, GitClient};
use serde::Serialize;

/// The manifest schema of `experiments/<id>/manifest.json`.
pub const MANIFEST_SCHEMA_VERSION: &str = "1.0.0";

/// Directories a walk for the checkout size skips: git's own data and
/// build output, which a fresh worktree does not have.
const NOT_CHECKED_OUT: [&str; 3] = [".git", "target", "node_modules"];

/// Read the git facts of `project`. A failing git call leaves its fact
/// unknown; PRE-01 then fails with the reason. `branches` are the planned
/// candidate branches; each one that exists already is "taken".
pub fn repo_facts(git: &GitCli, project: &Path, branches: &[String]) -> GitFacts {
    let toplevel = git.toplevel(project).ok();
    let dir = toplevel.as_deref().unwrap_or(project);
    let base_sha = toplevel.as_ref().and_then(|_| git.head(dir).ok());
    let dirty = toplevel.is_some()
        && git
            .status_porcelain(dir)
            .map(|s| !s.trim().is_empty())
            .unwrap_or(true);
    let worktree_supported = toplevel.is_some() && git.worktree_list(dir).is_ok();
    let namespace_taken = match &toplevel {
        Some(dir) => branches
            .iter()
            .filter(|b| {
                git.rev_parse(dir, &format!("refs/heads/{b}"))
                    .map(|sha| sha.is_some())
                    .unwrap_or(false)
            })
            .cloned()
            .collect(),
        None => Vec::new(),
    };
    GitFacts {
        toplevel,
        base_sha,
        dirty,
        worktree_supported,
        namespace_taken,
        git_version: git.version().ok().map(|v| redact(v.trim()).into_owned()),
    }
}

/// `--version` of every harness in `harnesses`, redacted (PRE-06, PRE-07).
/// No model turn: `--version` only. `None` when the binary is missing or
/// does not answer.
pub fn harness_versions(
    ctx: &RuntimeContext,
    harnesses: &BTreeSet<&'static str>,
) -> BTreeMap<String, Option<String>> {
    harnesses
        .iter()
        .map(|name| {
            let kind = kind_of(name);
            let version = match kind.and_then(|k| k.binary(&ctx.bins.harness)) {
                Some(bin) => harness_version(&bin).map(|v| redact(&v).into_owned()),
                // The smoke harness runs no agent, so it has nothing to resolve.
                None if kind == Some(HarnessKind::None) => Some("none".to_string()),
                None => None,
            };
            (name.to_string(), version)
        })
        .collect()
}

fn kind_of(name: &str) -> Option<HarnessKind> {
    [
        HarnessKind::Claude,
        HarnessKind::Codex,
        HarnessKind::OpenCode,
        HarnessKind::Pi,
        HarnessKind::Prime,
        HarnessKind::None,
    ]
    .into_iter()
    .find(|k| k.as_str() == name)
}

/// Each candidate's quota pool and its state, as the router sees it.
pub fn pool_facts(view: &QuotaView, candidates: &[PreflightCandidate]) -> Vec<PoolFacts> {
    let pools: BTreeSet<&str> = candidates
        .iter()
        .map(|c| pool_for(c.harness.as_str(), c.model.as_str()))
        .collect();
    pools
        .into_iter()
        .map(|pool| PoolFacts {
            pool: pool.to_string(),
            state: view
                .assess_pool(pool, None, None)
                .state
                .as_str()
                .to_string(),
        })
        .collect()
}

/// The inputs [`gather`] needs besides the context.
pub struct GatherInput<'a> {
    pub paths: &'a DatasetPaths,
    pub config: &'a DatasetConfig,
    pub candidates: Vec<PreflightCandidate>,
    pub git: GitFacts,
    pub view: &'a QuotaView,
}

/// Gather every fact of the plan, and the machine snapshot.
pub fn gather(ctx: &RuntimeContext, input: GatherInput<'_>) -> (PreflightPlan, MachineSnapshot) {
    let GatherInput {
        paths,
        config,
        candidates,
        git,
        view,
    } = input;
    // The judge model `opus` runs on Claude.
    let judge_harness = HarnessKind::Claude;
    let mut harnesses: BTreeSet<&'static str> =
        candidates.iter().map(|c| c.harness.as_str()).collect();
    harnesses.insert(judge_harness.as_str());
    let harness_versions = harness_versions(ctx, &harnesses);

    let storage = match paths.ensure() {
        Ok(()) => storage_probe(paths.root()),
        Err(_) => StorageProbe::default(),
    };
    let disk_dir = config
        .worktree_root
        .as_deref()
        .map(existing_ancestor)
        .unwrap_or_else(|| paths.root().to_path_buf());
    let snapshot = machine::probe(
        &disk_dir,
        &machine::ProbeBins::default(),
        ctx.settings.machine_file.as_deref(),
    );

    let checkout_bytes = git
        .toplevel
        .as_deref()
        .map(|t| tree_bytes(t, &NOT_CHECKED_OUT))
        .unwrap_or(0);
    // An existing build of the project is the best guess for one candidate's.
    // SPEC-TODO(Spec B §3): a build estimate for a project never built here.
    let build_bytes = git
        .toplevel
        .as_deref()
        .map(|t| tree_bytes(&t.join("target"), &[]))
        .unwrap_or(0);
    let n = candidates.len() as u64;
    let gates = config.gates.len().max(1) as u64;
    let artifacts_bytes = n * (config.caps.output_cap_bytes + config.caps.log_cap_bytes * gates);

    let pools = pool_facts(view, &candidates);
    let plan = PreflightPlan {
        config: config.clone(),
        candidates,
        harness_versions,
        judge_harness,
        git,
        storage_probe: storage,
        herdr_reachable: Herdr::with_bin(&ctx.bins.harness.herdr).server_reachable(),
        horch_exe: Some(ctx.bins.horch_exe.clone()).filter(|p| p.is_file()),
        pools,
        checkout_bytes,
        build_bytes,
        artifacts_bytes,
        // SPEC-TODO(Spec B §3): the local model size of a `pi` candidate.
        local_model_bytes: 0,
        expected_tokens: BTreeMap::new(),
        // SPEC-TODO(Spec B §3): the directories each harness trusts. With none,
        // PRE-13 warns for every worktree root.
        trusted_parents: Vec::new(),
    };
    (plan, snapshot)
}

/// `path`, or its nearest ancestor that exists: the disk a new directory
/// lands on.
fn existing_ancestor(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|p| p.exists())
        .unwrap_or(path)
        .to_path_buf()
}

/// Bytes of the regular files under `dir`, without following links and
/// without the directories named in `skip`. 0 when `dir` is missing.
fn tree_bytes(dir: &Path, skip: &[&str]) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.path().symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                if !skip.iter().any(|s| entry.file_name() == *s) {
                    stack.push(entry.path());
                }
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

/// The experiment's identity: what `experiment.created` and the manifest
/// carry.
pub struct ExperimentFacts<'a> {
    pub experiment: &'a ExperimentId,
    pub task: &'a str,
    pub plan: &'a PreflightPlan,
    pub report: &'a PreflightReport,
    /// The process variables the snapshot may keep; see `envsnap`.
    pub env: BTreeMap<String, String>,
    pub at: DateTime<Utc>,
}

/// `experiments/<id>/manifest.json`. No secret: the task is redacted, the
/// environment is the allowlisted snapshot, versions are redacted already.
#[derive(Debug, Serialize)]
struct Manifest<'a> {
    schema_version: &'static str,
    experiment_id: &'a ExperimentId,
    task_id: &'a TaskId,
    task_digest: &'a Digest,
    task: String,
    config: &'a DatasetConfig,
    config_digest: &'a Digest,
    repo_digest: &'a Digest,
    environment_digest: &'a Digest,
    harness_versions: &'a BTreeMap<String, Option<String>>,
    env_snapshot: BTreeMap<String, String>,
    preflight: &'a PreflightReport,
}

/// Record `experiment.created` and `preflight.completed`, write the
/// manifest, and on any Fail record `experiment.aborted`. Returns the
/// failed check ids.
pub fn record(
    recorder: &dyn Recorder,
    paths: &DatasetPaths,
    f: &ExperimentFacts<'_>,
) -> Result<Vec<String>> {
    let task_digest = sha256_bytes(f.task.as_bytes());
    let task_id = TaskId::new(format!("task-{}", task_digest.short12()))?;
    let config_digest = digest_json(&f.plan.config);
    let repo_digest = digest_json(&serde_json::json!({
        "toplevel": f.plan.git.toplevel,
        "base_sha": f.plan.git.base_sha,
    }));
    let exp = f.experiment;
    let event = |kind: EventKind, key: &str| NewEvent {
        kind,
        actor: Actor::Coordinator,
        experiment_id: exp.clone(),
        round_id: None,
        execution_id: None,
        idempotency_key: format!("{key}:{exp}"),
        occurred_at: f.at,
    };

    recorder.append(event(
        EventKind::ExperimentCreated(ExperimentCreated {
            task_id: task_id.clone(),
            task_digest: task_digest.clone(),
            config_digest: config_digest.clone(),
            base_sha: f.plan.git.base_sha.clone().unwrap_or_default(),
            repo_digest: repo_digest.clone(),
            environment_digest: f.report.environment_digest.clone(),
            candidates: f.plan.candidates.len() as u32,
            strategy: "diverse".to_string(),
            budget_usd_micro: f.plan.config.budget.hard_usd_micro,
            promote_to: f.plan.config.promote_to.clone(),
        }),
        "experiment.created",
    ))?;
    recorder.append(event(
        EventKind::PreflightCompleted(PreflightCompleted {
            report: f.report.clone(),
        }),
        "preflight.completed",
    ))?;

    let manifest = Manifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        experiment_id: exp,
        task_id: &task_id,
        task_digest: &task_digest,
        task: redact(f.task).into_owned(),
        config: &f.plan.config,
        config_digest: &config_digest,
        repo_digest: &repo_digest,
        environment_digest: &f.report.environment_digest,
        harness_versions: &f.plan.harness_versions,
        env_snapshot: env_snapshot(&f.env),
        preflight: f.report,
    };
    let dir = paths.experiment_dir(exp)?;
    fsx::ensure_private_dir(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let json = serde_json::to_vec_pretty(&manifest)?;
    let file = paths.manifest(exp)?;
    fsx::create_immutable(&file, &json, 0o600)
        .with_context(|| format!("writing {}", file.display()))?;

    let failed: Vec<String> = f
        .report
        .checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .map(|c| c.id.clone())
        .collect();
    if !failed.is_empty() {
        recorder.append(event(
            EventKind::ExperimentAborted(ExperimentAborted {
                reason: "preflight failed".to_string(),
                failed_checks: failed.clone(),
            }),
            "experiment.aborted",
        ))?;
    }
    Ok(failed)
}

/// The report as text: one line per check, then the totals.
pub fn render(report: &PreflightReport) -> String {
    let mut out = String::from("preflight:\n");
    for c in &report.checks {
        let status = match c.status {
            CheckStatus::Pass => "pass",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
        };
        out.push_str(&format!("  {} {status:<4} {}\n", c.id, c.detail));
    }
    out.push_str(&format!(
        "  safe N {}, {} wave(s), projected cost ${}.{:06}\n",
        report.safe_n,
        report.waves,
        report.projected_cost_microusd.0 / 1_000_000,
        report.projected_cost_microusd.0 % 1_000_000
    ));
    out.push_str(&format!(
        "  environment digest {}\n",
        report.environment_digest.short12()
    ));
    out
}
