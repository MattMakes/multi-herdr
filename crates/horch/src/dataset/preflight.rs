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
use horch_core::competition::budget::{measured_from_runs, MeasuredTokens};
use horch_core::competition::config::DatasetConfig;
use horch_core::competition::observe::load_usage_records;
use horch_core::competition::preflight::{
    storage_probe, CheckResult, CheckStatus, GitFacts, PoolFacts, PreflightCandidate,
    PreflightPlan, PreflightReport, StorageProbe,
};
use horch_core::fsx;
use horch_core::harness::trust::{
    asks_for_trust, ensure_trusted, read_trust, trust_note, HarnessTrust,
};
use horch_core::harness::HarnessKind;
use horch_core::ids::{ExperimentId, TaskId};
use horch_core::measure::digest::{digest_json, sha256_bytes, Digest};
use horch_core::measure::envsnap::env_snapshot;
use horch_core::measure::event::{
    Actor, EventKind, ExperimentAborted, ExperimentCreated, PreflightCompleted,
};
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::recorder::{NewEvent, Recorder};
use horch_core::measure::redact::redact;
use horch_core::measure::{projection, store};
use horch_core::routing::quota::{pool_for, QuotaView};
use horch_core::routing::quota_probe::harness_version;
use horch_core::runtime::fault::Faults;
use horch_core::runtime::machine::{self, MachineSnapshot};
use horch_core::runtime::RuntimeContext;
use horch_core::vcs::git::{GitCli, GitClient};
use horch_core::workspace::herdr::Herdr;
use serde::Serialize;

/// The manifest schema of `experiments/<id>/manifest.json`.
pub(crate) const MANIFEST_SCHEMA_VERSION: &str = "1.0.0";

/// Directories a walk for the checkout size skips: git's own data and
/// build output, which a fresh worktree does not have.
const NOT_CHECKED_OUT: [&str; 3] = [".git", "target", "node_modules"];

/// Read the git facts of `project`. A failing git call leaves its fact
/// unknown; PRE-01 then fails with the reason. `branches` are the planned
/// candidate branches; each one that exists already is "taken".
/// `promote_to` is the `--promote-to` target, checked to be a local branch
/// outside the candidate namespace.
pub(crate) fn repo_facts(
    git: &GitCli,
    project: &Path,
    branches: &[String],
    promote_to: Option<&str>,
) -> GitFacts {
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
    let promote_target_problem = match (&toplevel, promote_to) {
        (Some(dir), Some(target)) => promote_target_problem(git, dir, target),
        _ => None,
    };
    GitFacts {
        toplevel,
        base_sha,
        dirty,
        worktree_supported,
        namespace_taken,
        promote_target_problem,
        git_version: git.version().ok().map(|v| redact(v.trim()).into_owned()),
    }
}

/// The namespace of the branches a competition round may create: the
/// promotion makes `compete/<slug>` at the winner's commit (fleet-dataset
/// §7.3).
const NEW_BRANCH_PREFIX: &str = "compete/";

/// Why `target` cannot take a promotion, or `None` when it can: it must
/// be an existing local branch and not a candidate branch, or a valid
/// `compete/<slug>` branch that does not exist yet.
fn promote_target_problem(git: &GitCli, dir: &Path, target: &str) -> Option<String> {
    if target.starts_with("mh/exp/") {
        return Some(format!(
            "the promotion target '{target}' is a candidate branch (mh/exp/...)"
        ));
    }
    match git.rev_parse(dir, &format!("refs/heads/{target}")) {
        Ok(Some(_)) => None,
        Ok(None) if target.starts_with(NEW_BRANCH_PREFIX) && branch_name_ok(target) => None,
        _ => Some(format!(
            "the promotion target '{target}' is not a local branch \
             (only a new {NEW_BRANCH_PREFIX}<slug> branch may be created)"
        )),
    }
}

/// `name` can name a branch, by the rules of `git check-ref-format`.
fn branch_name_ok(name: &str) -> bool {
    let bad_char = |c: char| c.is_ascii_control() || " ~^:?*[\\".contains(c);
    !name.is_empty()
        && !name.contains("..")
        && !name.contains("@{")
        && !name.chars().any(bad_char)
        && !name.ends_with('.')
        && name
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

/// Why the `--plan` file cannot brief the candidates, or `None` (FDS-09).
/// The candidates start from `base`, so the plan must be tracked there and
/// the work tree must hold the same bytes. `plan` is relative to `dir`, the
/// repo root, `/`-separated.
pub(crate) fn plan_problem(
    git: &GitCli,
    dir: &Path,
    base: Option<&str>,
    plan: &str,
) -> Option<String> {
    let fix = format!("commit {plan} first");
    let tracked = base
        .is_some_and(|base| matches!(git.rev_parse(dir, &format!("{base}:{plan}")), Ok(Some(_))));
    if !tracked {
        return Some(format!(
            "the plan {plan} is not tracked at the base commit: {fix}"
        ));
    }
    // The base is HEAD, so `status` lists the plan when it differs.
    let changed = git
        .status_porcelain(dir)
        .map(|s| s.lines().any(|l| porcelain_path(l) == plan))
        .unwrap_or(true);
    changed.then(|| format!("the plan {plan} differs from the base commit: {fix}"))
}

/// The path of one `git status --porcelain` line: after the status letters,
/// the new name of a rename, unquoted.
fn porcelain_path(line: &str) -> &str {
    let path = line
        .trim_start()
        .split_once(' ')
        .map_or("", |(_, p)| p.trim_start());
    let path = path.rsplit_once(" -> ").map_or(path, |(_, new)| new);
    path.strip_prefix('"')
        .and_then(|p| p.strip_suffix('"'))
        .unwrap_or(path)
}

/// Fail PRE-01, the git check, with `problem` as well: a plan that the
/// base commit does not hold is a git fact (FDS-09).
pub(crate) fn fail_pre_01(report: &mut PreflightReport, problem: &str) {
    if let Some(c) = report.checks.iter_mut().find(|c| c.id == "PRE-01") {
        c.detail = match c.status {
            CheckStatus::Fail => format!("{}; {problem}", c.detail),
            _ => problem.to_string(),
        };
        c.status = CheckStatus::Fail;
        if let Some(m) = c.measured.as_object_mut() {
            m.insert("plan_problem".to_string(), problem.into());
        }
    } else {
        report.checks.insert(
            0,
            CheckResult {
                id: "PRE-01".to_string(),
                status: CheckStatus::Fail,
                detail: problem.to_string(),
                measured: serde_json::json!({ "plan_problem": problem }),
            },
        );
    }
    report.passed = false;
}

/// `--version` of every harness in `harnesses`, redacted (PRE-06, PRE-07).
/// No model turn: `--version` only. `None` when the binary is missing or
/// does not answer.
pub(crate) fn harness_versions(
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
    HarnessKind::ALL
        .iter()
        .copied()
        .find(|k| k.as_str() == name)
}

/// Each candidate's quota pool and its state, as the router sees it.
pub(crate) fn pool_facts(view: &QuotaView, candidates: &[PreflightCandidate]) -> Vec<PoolFacts> {
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
pub(crate) struct GatherInput<'a> {
    pub paths: &'a DatasetPaths,
    /// The task text: PRE-09 reads the measured usage of its earlier runs.
    pub task: &'a str,
    pub config: &'a DatasetConfig,
    pub candidates: Vec<PreflightCandidate>,
    pub git: GitFacts,
    pub view: &'a QuotaView,
}

/// Gather every fact of the plan, and the machine snapshot.
pub(crate) fn gather(
    ctx: &RuntimeContext,
    input: GatherInput<'_>,
) -> (PreflightPlan, MachineSnapshot) {
    let GatherInput {
        paths,
        task,
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
    // A project never built here counts 0; the disk headroom covers its first
    // build (dataset design 4.11.1, decision 1).
    let build_bytes = git
        .toplevel
        .as_deref()
        .map(|t| tree_bytes(&t.join("target"), &[]))
        .unwrap_or(0);
    let n = candidates.len() as u64;
    let gates = config.gates.len().max(1) as u64;
    let artifacts_bytes = n * (config.caps.output_cap_bytes + config.caps.log_cap_bytes * gates);

    let pools = pool_facts(view, &candidates);
    let trust_root = git.toplevel.as_deref().map(main_root);
    let trust = trust_root
        .as_deref()
        .map(|root| harness_trust(ctx, &candidates, root))
        .unwrap_or_default();
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
        // No probe knows a `pi` candidate's model size; with 0, local
        // candidates run 1 at a time (dataset design 4.11.1, decision 2).
        local_model_bytes: 0,
        expected_tokens: BTreeMap::new(),
        measured_tokens: measured_tokens(paths, task),
        trust_root,
        trust,
    };
    (plan, snapshot)
}

/// The main repository root of `toplevel`: for a linked worktree, the
/// directory that holds the common `.git`. Claude and Codex key their trust
/// on it (dataset design 4.11.1, PRE-14).
fn main_root(toplevel: &Path) -> PathBuf {
    std::fs::read_to_string(toplevel.join(".git"))
        .ok()
        .and_then(|text| {
            let gitdir = text.trim().strip_prefix("gitdir:")?.trim().to_string();
            let cut = gitdir.find("/.git/worktrees/")?;
            Some(PathBuf::from(&gitdir[..cut]))
        })
        .unwrap_or_else(|| toplevel.to_path_buf())
}

/// What each candidate harness that asks for trust has recorded for
/// `root` (PRE-14), after horch wrote each missing trust entry (ARC-29), so
/// PRE-14 fails only when that write was skipped or failed. It reads the
/// stores and keeps only the verdict: the files can hold tokens, so nothing
/// else of them leaves this function.
fn harness_trust(
    ctx: &RuntimeContext,
    candidates: &[PreflightCandidate],
    root: &Path,
) -> Vec<HarnessTrust> {
    let mut roots = vec![root.to_path_buf()];
    if let Ok(canonical) = root.canonicalize() {
        if canonical != root {
            roots.push(canonical);
        }
    }
    let mut harnesses: Vec<HarnessKind> = candidates
        .iter()
        .map(|c| c.harness)
        .filter(|h| asks_for_trust(*h))
        .collect();
    harnesses.sort_unstable_by_key(|h| h.as_str());
    harnesses.dedup();
    for &harness in &harnesses {
        let outcome = ensure_trusted(
            harness,
            root,
            &ctx.paths.home,
            ctx.inherited.claude_config_dir.as_deref(),
            ctx.inherited.codex_home.as_deref(),
        );
        if let Some(line) = trust_note(harness, root, &outcome) {
            eprintln!("{line}");
        }
    }
    read_trust(
        &harnesses,
        &ctx.paths.home,
        ctx.inherited.claude_config_dir.as_deref(),
        ctx.inherited.codex_home.as_deref(),
        &roots,
    )
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
pub(crate) struct ExperimentFacts<'a> {
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

/// What the earlier candidates of `task` used, per model, for PRE-09: each
/// candidate of a round of the same task id that has a usage record. A
/// dataset that cannot be read gives no measurement, and PRE-09 uses the
/// next source.
fn measured_tokens(paths: &DatasetPaths, task: &str) -> BTreeMap<String, MeasuredTokens> {
    let (Ok(task_id), Ok(read)) = (task_id_of(task), store::read_all(paths)) else {
        return BTreeMap::new();
    };
    let view = projection::fold(&read.events);
    let usage = load_usage_records(paths);
    // Round ids are UUIDv7: the map's order is the order of creation.
    let runs = view
        .rounds
        .values()
        .filter(|r| {
            view.experiments
                .get(&r.experiment_id)
                .is_some_and(|e| e.created.task_id == task_id)
        })
        .flat_map(|r| r.candidates.values())
        .filter_map(|c| {
            let planned = c.planned.as_ref()?;
            let record = usage.get(c.execution_id.as_ref()?.as_str())?;
            Some((planned.model.as_str().to_string(), record.tokens.into()))
        });
    measured_from_runs(runs)
}

/// The task id of `task`: `task-` and 12 hex digits of its SHA-256. Every
/// run of the same text has the same id.
fn task_id_of(task: &str) -> Result<TaskId> {
    let digest = sha256_bytes(task.as_bytes());
    Ok(TaskId::new(format!("task-{}", digest.short12()))?)
}

/// Record `experiment.created` and `preflight.completed`, write the
/// manifest, and on any Fail record `experiment.aborted`. Returns the
/// failed check ids.
pub(crate) fn record(
    recorder: &dyn Recorder,
    paths: &DatasetPaths,
    f: &ExperimentFacts<'_>,
    faults: &Faults,
) -> Result<Vec<String>> {
    let task_digest = sha256_bytes(f.task.as_bytes());
    let task_id = task_id_of(f.task)?;
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
            task_digest,
            config_digest,
            base_sha: f.plan.git.base_sha.clone().unwrap_or_default(),
            repo_digest,
            environment_digest: f.report.environment_digest,
            candidates: f.plan.candidates.len() as u32,
            strategy: "diverse".to_string(),
            budget_usd_micro: f.plan.config.budget.hard_usd_micro,
            promote_to: f.plan.config.promote_to.clone(),
        }),
        "experiment.created",
    ))?;
    faults.abort_if("abort-after-experiment-created");
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
pub(crate) fn render(report: &PreflightReport) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::harness::trust::TrustState;

    /// PRE-09's measured source reads the usage records of the earlier
    /// candidates of the same task id, per model; another task's runs and a
    /// candidate without a usage record do not count.
    #[test]
    fn measured_tokens_reads_the_same_task_only() {
        use horch_core::ids::{ExecutionId, ModelId, RoundId, TeammateName};
        use horch_core::measure::event::{
            Actor, CandidatePlanned, EventKind, RoundCreated, SlotKind,
        };
        use horch_core::measure::recorder::{JsonlRecorder, NewEvent};
        use horch_core::measure::store::StoreOptions;
        use horch_core::teacher::TeacherRef;

        let tmp = tempfile::tempdir().unwrap();
        let paths = DatasetPaths::from_slug(tmp.path(), "fixture");
        let rec = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();
        let uuid = |n: u64| format!("0199a5b0-0000-7000-8000-{n:012x}");
        let at = Utc::now();
        // A passed report: `round.created` needs a PLANNED experiment.
        let report: PreflightReport = serde_json::from_value(serde_json::json!({
            "schema_version": "1.0.0", "checks": [], "safe_n": 1, "waves": 1,
            "projected_cost_microusd": 0,
            "machine": {"os": "macos", "arch": "aarch64", "cpus": 10,
                        "mem_total_bytes": null, "mem_available_bytes": null,
                        "disk_free_bytes": null, "disk_total_bytes": null,
                        "gpu": "apple_silicon", "max_open_files": null, "max_processes": null},
            "environment_digest": sha256_bytes(b"e").to_string(),
            "passed": true,
        }))
        .unwrap();
        let mut n = 0u64;
        let mut push =
            |exp: &ExperimentId, round: Option<&RoundId>, exec: Option<ExecutionId>, kind| {
                n += 1;
                rec.append(NewEvent {
                    kind,
                    actor: Actor::Coordinator,
                    experiment_id: exp.clone(),
                    round_id: round.cloned(),
                    execution_id: exec,
                    idempotency_key: format!("k{n}"),
                    occurred_at: at,
                })
                .unwrap();
            };
        // 3 rounds of "fix the bug", 1 round of another task. Round 2's
        // candidate has no usage record.
        for (i, task) in [
            "fix the bug",
            "fix the bug",
            "fix the bug",
            "fix the bug",
            "other",
        ]
        .into_iter()
        .enumerate()
        {
            let i = i as u64;
            let exp = ExperimentId::new(uuid(0xe00 + i)).unwrap();
            let round = RoundId::new(uuid(0xa00 + i)).unwrap();
            let exec = ExecutionId::new(uuid(0xc00 + i)).unwrap();
            let task_id = task_id_of(task).unwrap();
            push(
                &exp,
                None,
                None,
                EventKind::ExperimentCreated(ExperimentCreated {
                    task_id,
                    task_digest: sha256_bytes(task.as_bytes()),
                    config_digest: sha256_bytes(b"c"),
                    base_sha: "a".repeat(40),
                    repo_digest: sha256_bytes(b"r"),
                    environment_digest: sha256_bytes(b"e"),
                    candidates: 1,
                    strategy: "diverse".into(),
                    budget_usd_micro: 2_000_000,
                    promote_to: None,
                }),
            );
            push(
                &exp,
                None,
                None,
                EventKind::PreflightCompleted(PreflightCompleted {
                    report: report.clone(),
                }),
            );
            push(
                &exp,
                Some(&round),
                None,
                EventKind::RoundCreated(RoundCreated {
                    index: 0,
                    base_sha: "a".repeat(40),
                    labels: vec!["A".into()],
                    eligible_set: Vec::new(),
                    propensities: BTreeMap::new(),
                    teacher: TeacherRef::none(),
                    seed: 1,
                    label_policy_version: "lp-1".into(),
                }),
            );
            push(
                &exp,
                Some(&round),
                Some(exec.clone()),
                EventKind::CandidatePlanned(CandidatePlanned {
                    label: "A".into(),
                    teammate: TeammateName::new("sonnet").unwrap(),
                    harness: HarnessKind::Claude,
                    model: ModelId::new("sonnet").unwrap(),
                    effort: None,
                    slot: SlotKind::Baseline,
                    propensity: 1.0,
                    config_id: "sonnet".into(),
                }),
            );
            if i == 2 {
                continue;
            }
            let dir = paths.artifacts_dir(&exp, &round).unwrap().join("usage");
            std::fs::create_dir_all(&dir).unwrap();
            let record = serde_json::json!({
                "execution_id": exec, "label": "A",
                "tokens": {"input": 100 * (i + 1), "cache_write_5m": 0, "cache_write_1h": 7,
                           "cache_read": 1000, "output": 10},
                "cost_microusd": 1, "cost_source": "unpriced",
                "transcript_ref": null, "transcript_digest": null,
            });
            std::fs::write(dir.join("A.json"), record.to_string()).unwrap();
        }

        let m = measured_tokens(&paths, "fix the bug");
        assert_eq!(m.len(), 1);
        assert_eq!(m["sonnet"].runs, 3);
        // Rounds 0, 1 and 3; the "other" task's 500 input is not counted.
        assert_eq!(m["sonnet"].estimate.input, 400);
        assert_eq!(m["sonnet"].estimate.cache_write_1h, 7);
        assert!(measured_tokens(&paths, "never run").is_empty());
    }

    /// Only a `compete/` branch may be new, and only with a name git takes.
    #[test]
    fn branch_names_follow_check_ref_format() {
        for ok in ["compete/greeting", "compete/f2-run", "compete/a.b"] {
            assert!(branch_name_ok(ok), "{ok}");
        }
        for bad in [
            "compete/",
            "compete//x",
            "compete/.x",
            "compete/x.lock",
            "compete/a..b",
            "compete/a b",
            "compete/a:b",
            "compete/a@{1}",
            "compete/x.",
        ] {
            assert!(!branch_name_ok(bad), "{bad}");
        }
    }

    /// Every harness kind resolves by name, so preflight shows its version.
    #[test]
    fn kind_of_names_every_harness() {
        for kind in HarnessKind::ALL {
            assert_eq!(kind_of(kind.as_str()), Some(*kind));
        }
        assert_eq!(kind_of("antigravity"), Some(HarnessKind::Antigravity));
        assert_eq!(kind_of("nope"), None);
    }

    /// PRE-14 reads Claude's trust from `$CLAUDE_CONFIG_DIR/.claude.json`
    /// when the variable is set, and from `~/.claude.json` otherwise.
    #[test]
    fn claude_trust_follows_claude_config_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        let home = tmp.path().join("home");
        let ccd = tmp.path().join("ccd");
        for dir in [&root, &home, &ccd] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let trusted = serde_json::json!({"projects": {
            root.to_string_lossy(): {"hasTrustDialogAccepted": true}
        }})
        .to_string();
        let candidates = [PreflightCandidate {
            label: "A".into(),
            teammate: horch_core::ids::TeammateName::new("sonnet").unwrap(),
            harness: HarnessKind::Claude,
            model: horch_core::ids::ModelId::new("sonnet").unwrap(),
            effort: None,
        }];
        let state = |with_ccd: bool| {
            let mut env =
                horch_core::runtime::MapEnv::new(&root).with("HOME", &home.to_string_lossy());
            if with_ccd {
                env = env.with("CLAUDE_CONFIG_DIR", &ccd.to_string_lossy());
            }
            let ctx = RuntimeContext::from_env(&env).unwrap();
            harness_trust(&ctx, &candidates, &root)[0].state
        };
        // Only the config dir trusts the root.
        std::fs::write(ccd.join(".claude.json"), &trusted).unwrap();
        assert_eq!(state(true), TrustState::Trusted);
        assert_eq!(state(false), TrustState::Untrusted);
        // Only the home file trusts it.
        std::fs::remove_file(ccd.join(".claude.json")).unwrap();
        std::fs::write(home.join(".claude.json"), &trusted).unwrap();
        assert_eq!(state(true), TrustState::Untrusted);
        assert_eq!(state(false), TrustState::Trusted);
    }

    /// ARC-29 and PRE-14: the coordinator writes a missing trust entry for
    /// the main repository root before the check reads it, so PRE-14 fails
    /// only when the write was skipped: here, when Claude never ran with
    /// this config dir.
    #[test]
    fn pre_14_trust_is_written_before_the_check() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = base.join("repo");
        let home = base.join("home");
        for dir in [&root, &home.join(".codex")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let candidates: Vec<PreflightCandidate> = [
            ("A", "sonnet", HarnessKind::Claude),
            ("B", "codex-terra", HarnessKind::Codex),
        ]
        .into_iter()
        .map(|(label, teammate, harness)| PreflightCandidate {
            label: label.into(),
            teammate: horch_core::ids::TeammateName::new(teammate).unwrap(),
            harness,
            model: horch_core::ids::ModelId::new("m").unwrap(),
            effort: None,
        })
        .collect();
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new(&root).with("HOME", &home.to_string_lossy()),
        )
        .unwrap();
        let states = || -> Vec<(String, TrustState)> {
            harness_trust(&ctx, &candidates, &root)
                .into_iter()
                .map(|t| (t.harness, t.state))
                .collect()
        };
        // No .claude.json: Claude is skipped; Codex's file is created.
        assert_eq!(
            states(),
            [
                ("claude".to_string(), TrustState::Untrusted),
                ("codex".to_string(), TrustState::Trusted)
            ]
        );
        std::fs::write(home.join(".claude.json"), r#"{"numStartups": 1}"#).unwrap();
        assert_eq!(
            states(),
            [
                ("claude".to_string(), TrustState::Trusted),
                ("codex".to_string(), TrustState::Trusted)
            ]
        );
    }

    /// A linked worktree resolves to the main repository root, where the
    /// harnesses keep their trust; a main checkout is its own root.
    #[test]
    fn main_root_of_a_linked_worktree_is_the_main_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("app");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        assert_eq!(main_root(&main), main);
        let wt = tmp.path().join("wt-a");
        std::fs::create_dir_all(&wt).unwrap();
        let gitdir = format!("gitdir: {}/.git/worktrees/wt-a\n", main.display());
        std::fs::write(wt.join(".git"), gitdir).unwrap();
        assert_eq!(main_root(&wt), main);
        // A submodule's `.git` file is not a worktree: it stays its own root.
        let sub = tmp.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(".git"), "gitdir: ../app/.git/modules/sub\n").unwrap();
        assert_eq!(main_root(&sub), sub);
    }

    /// A project with no `target/` directory counts 0 build bytes, and the
    /// checkout skips `.git`, `target` and `node_modules` (dataset design
    /// 4.11.1, facts and decision 1).
    #[test]
    fn tree_bytes_counts_a_never_built_project_as_zero_build() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("main.rs"), b"fn main() {}\n").unwrap();
        for skipped in NOT_CHECKED_OUT {
            std::fs::create_dir_all(root.join(skipped)).unwrap();
            std::fs::write(root.join(skipped).join("big"), vec![0u8; 4096]).unwrap();
        }
        assert_eq!(tree_bytes(root, &NOT_CHECKED_OUT), 13);
        assert_eq!(tree_bytes(&root.join("target"), &[]), 4096);
        assert_eq!(tree_bytes(&root.join("no-such-target"), &[]), 0);
    }
}
