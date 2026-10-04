//! Preflight: can this machine run this round, and how many candidates at once?
//!
//! [`evaluate`] is pure. The caller gathers the facts (machine snapshot, git,
//! harness versions, the storage probe) and gets one [`CheckResult`] per
//! PRE id, PRE-01..PRE-14 in order, plus the safe parallelism, the number of
//! waves and the projected cost. A run whose report has any `Fail` is refused
//! before a worktree is created or a model is invoked.
//!
//! The check list and every threshold are specified in the dataset design,
//! section 4.11.1 (Spec B §3). Each threshold is a named constant.

use crate::harness::capabilities::HARNESS_FOOTPRINT_BYTES;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::competition::budget::{estimate_cost, resolve_estimate, EstimateSource, MeasuredTokens};
use crate::competition::config::{DatasetConfig, ExpectedTokens};
use crate::fsx::DirLock;
use crate::harness::trust::{asks_for_trust, trust_fix, HarnessTrust, TrustState};
use crate::harness::HarnessKind;
use crate::ids::{ModelId, TeammateName};
use crate::measure::digest::{digest_json, Digest};
use crate::runtime::machine::{GpuClass, Known, MachineSnapshot};
use crate::usage::money::{MicroUsd, NanoUsd};
use crate::usage::{builtin_prices, Price, Tokens};

pub(crate) const REPORT_SCHEMA_VERSION: &str = "1.0.0";

/// The oldest git with `git worktree` (added in 2.5, stable with `--lock`
/// and `remove` by 2.17).
pub(crate) const MIN_GIT_VERSION: (u32, u32) = (2, 17);
/// CPU cores one candidate needs: the agent plus its builds and tests.
pub(crate) const CPUS_PER_CANDIDATE: u32 = 2;
/// Open files one candidate needs (agent, pane, builds, logs).
pub(crate) const FDS_PER_CANDIDATE: u64 = 256;
/// Processes one candidate needs.
pub(crate) const PROCS_PER_CANDIDATE: u64 = 64;
/// Tokens one candidate is expected to use when the plan has no estimate.
/// The last of PRE-09's sources (see [`crate::competition::budget::resolve_estimate`]).
pub(crate) const DEFAULT_TOKEN_ESTIMATE: TokenEstimate = TokenEstimate {
    input: 200_000,
    cache_write_5m: 0,
    cache_write_1h: 0,
    cache_read: 3_000_000,
    output: 60_000,
};

/// A candidate the round plans to run.
// B3: replaced by CandidatePlanned (U13) once it lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightCandidate {
    pub label: String,
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: ModelId,
    #[serde(default)]
    pub effort: Option<String>,
}

/// One quota pool and its state, as the router reports it (`"ok"`,
/// `"limited"`, `"exhausted"`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolFacts {
    pub pool: String,
    pub state: String,
}

/// What git says about the project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFacts {
    pub toplevel: Option<PathBuf>,
    pub base_sha: Option<String>,
    pub dirty: bool,
    pub worktree_supported: bool,
    /// Planned branch names that already exist.
    pub namespace_taken: Vec<String>,
    /// Why `--promote-to` names no valid target: it is not a local branch,
    /// or it is a candidate branch (`mh/exp/...`). `None` when it is valid
    /// or not asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promote_target_problem: Option<String>,
    /// `git --version` output, such as `git version 2.39.5`.
    pub git_version: Option<String>,
}

/// The result of [`storage_probe`] on the dataset directory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageProbe {
    pub writable: bool,
    pub lock_ok: bool,
    pub fsync_ok: bool,
    pub rename_ok: bool,
}

/// Expected tokens for one candidate, in the 5 kinds [`crate::usage::Price`]
/// prices. A kind left out of `dataset.yaml` is 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TokenEstimate {
    pub input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub output: u64,
}

impl TokenEstimate {
    /// No token of any kind.
    pub fn is_zero(&self) -> bool {
        *self == TokenEstimate::default()
    }
}

impl From<Tokens> for TokenEstimate {
    fn from(t: Tokens) -> Self {
        TokenEstimate {
            input: t.input,
            cache_write_5m: t.cache_write_5m,
            cache_write_1h: t.cache_write_1h,
            cache_read: t.cache_read,
            output: t.output,
        }
    }
}

/// Everything [`evaluate`] looks at besides the machine.
#[derive(Debug, Clone, PartialEq)]
pub struct PreflightPlan {
    pub config: DatasetConfig,
    pub candidates: Vec<PreflightCandidate>,
    /// Resolved version per harness, keyed by [`HarnessKind::as_str`];
    /// `None` when the binary was not found or did not answer.
    pub harness_versions: BTreeMap<String, Option<String>>,
    /// The harness the judge runs on (the judge model `opus` runs on Claude).
    pub judge_harness: HarnessKind,
    pub git: GitFacts,
    pub storage_probe: StorageProbe,
    pub herdr_reachable: bool,
    pub horch_exe: Option<PathBuf>,
    pub pools: Vec<PoolFacts>,
    /// Size of one worktree checkout.
    pub checkout_bytes: u64,
    /// Build output of one candidate (its own `CARGO_TARGET_DIR` and the like).
    pub build_bytes: u64,
    /// Logs, diffs and transcripts of the whole round.
    pub artifacts_bytes: u64,
    /// Memory of the local model a `pi` candidate loads.
    pub local_model_bytes: u64,
    /// Expected tokens per candidate label. It wins over every other source
    /// of PRE-09's estimate; the CLI leaves it empty.
    pub expected_tokens: BTreeMap<String, TokenEstimate>,
    /// What earlier candidates of the same task used, per model
    /// ([`crate::competition::budget::measured_estimates`]).
    pub measured_tokens: BTreeMap<String, MeasuredTokens>,
    /// The main repository root: the harnesses key their trust on it, also
    /// for a linked worktree.
    pub trust_root: Option<PathBuf>,
    /// What each candidate harness that asks for trust has recorded for
    /// `trust_root` (PRE-14).
    pub trust: Vec<HarnessTrust>,
}

impl PreflightPlan {
    fn version_of(&self, harness: HarnessKind) -> Option<&str> {
        self.harness_versions
            .get(harness.as_str())
            .and_then(|v| v.as_deref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    /// `"PRE-01"` .. `"PRE-14"`.
    pub id: String,
    pub status: CheckStatus,
    pub detail: String,
    pub measured: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreflightReport {
    pub schema_version: String,
    /// PRE-01..PRE-14, in order.
    pub checks: Vec<CheckResult>,
    pub safe_n: u32,
    pub waves: u32,
    pub projected_cost_microusd: MicroUsd,
    pub machine: MachineSnapshot,
    pub environment_digest: Digest,
    /// No check failed.
    pub passed: bool,
}

impl PreflightReport {
    /// The check with `id`, such as `"PRE-09"`.
    pub fn check(&self, id: &str) -> Option<&CheckResult> {
        self.checks.iter().find(|c| c.id == id)
    }
}

fn check(
    id: &str,
    status: CheckStatus,
    detail: impl Into<String>,
    measured: serde_json::Value,
) -> CheckResult {
    CheckResult {
        id: id.to_string(),
        status,
        detail: detail.into(),
        measured,
    }
}

/// Evaluate `plan` against `snapshot`. Pure.
pub fn evaluate(plan: &PreflightPlan, snapshot: &MachineSnapshot) -> PreflightReport {
    let n = plan.candidates.len() as u32;
    let bounds = Bounds::of(plan, snapshot);
    let safe_n = bounds.safe_n(n);
    let waves = n.div_ceil(safe_n);
    let cost = project_cost(plan, &builtin_prices());

    let checks = vec![
        pre_01_git(plan),
        pre_02_disk(plan, snapshot, n),
        pre_03_memory(plan, snapshot, safe_n),
        pre_04_cpu_gpu(plan, snapshot, &bounds),
        pre_05_limits(snapshot, &bounds, n),
        harness_check(
            "PRE-06",
            plan,
            "every candidate harness resolved before worktrees",
        ),
        pre_07_providers(plan),
        pre_08_safe_n(n, safe_n, waves, &bounds),
        pre_09_budget(plan, &cost),
        pre_10_judge(plan),
        pre_11_storage(plan.storage_probe),
        harness_check("PRE-12", plan, "the round can start without refusal"),
        pre_13_herdr(plan),
        pre_14_trust(plan),
    ];
    let passed = checks.iter().all(|c| c.status != CheckStatus::Fail);
    let environment_digest = digest_json(&json!({
        "machine": snapshot,
        "harness_versions": plan.harness_versions,
        "git_version": plan.git.git_version,
    }));
    PreflightReport {
        schema_version: REPORT_SCHEMA_VERSION.to_string(),
        checks,
        safe_n,
        waves,
        projected_cost_microusd: cost.total,
        machine: snapshot.clone(),
        environment_digest,
        passed,
    }
}

// ─── PRE-01 git ─────────────────────────────────────────────────────────────

fn pre_01_git(plan: &PreflightPlan) -> CheckResult {
    let git = &plan.git;
    let version = git.git_version.as_deref().and_then(parse_git_version);
    let mut problems = Vec::new();
    if git.toplevel.is_none() {
        problems.push("the project is not inside a git repository".to_string());
    }
    if git.base_sha.is_none() {
        problems.push("no base commit (HEAD does not resolve)".to_string());
    }
    if git.dirty && !plan.config.allow_dirty {
        problems.push("the working tree is dirty (commit, or pass --allow-dirty)".to_string());
    }
    match version {
        _ if !git.worktree_supported => {
            problems.push("git worktrees are not supported".to_string())
        }
        None => problems.push("the git version is unknown".to_string()),
        Some(v) if v < MIN_GIT_VERSION => problems.push(format!(
            "git {}.{} is older than {}.{}",
            v.0, v.1, MIN_GIT_VERSION.0, MIN_GIT_VERSION.1
        )),
        Some(_) => {}
    }
    if !git.namespace_taken.is_empty() {
        problems.push(format!(
            "branch names already exist: {}",
            git.namespace_taken.join(", ")
        ));
    }
    if let Some(problem) = &git.promote_target_problem {
        problems.push(problem.clone());
    }
    let measured = json!({
        "toplevel": git.toplevel,
        "base_sha": git.base_sha,
        "dirty": git.dirty,
        "allow_dirty": plan.config.allow_dirty,
        "worktree_supported": git.worktree_supported,
        "git_version": git.git_version,
        "namespace_taken": git.namespace_taken,
        "promote_target_problem": git.promote_target_problem,
    });
    if problems.is_empty() {
        let detail = if git.dirty {
            "git ready (dirty tree allowed by --allow-dirty)"
        } else {
            "git ready"
        };
        check("PRE-01", CheckStatus::Pass, detail, measured)
    } else {
        check("PRE-01", CheckStatus::Fail, problems.join("; "), measured)
    }
}

/// `(major, minor)` from `git version 2.39.5 (Apple Git-154)` or `2.39.5`.
pub fn parse_git_version(text: &str) -> Option<(u32, u32)> {
    let token = text
        .split_whitespace()
        .find(|t| t.starts_with(|c: char| c.is_ascii_digit()))?;
    let mut parts = token.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()
        .map(|m| {
            m.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    Some((major, minor))
}

// ─── PRE-02 disk ────────────────────────────────────────────────────────────

fn pre_02_disk(plan: &PreflightPlan, snapshot: &MachineSnapshot, n: u32) -> CheckResult {
    // Every worktree stays on disk until cleanup, so all N count, not one wave.
    let need = u64::from(n)
        .saturating_mul(plan.checkout_bytes.saturating_add(plan.build_bytes))
        .saturating_add(plan.artifacts_bytes)
        .saturating_add(plan.config.caps.disk_headroom_bytes);
    let mut measured = json!({
        "need_bytes": need,
        "candidates": n,
        "checkout_bytes": plan.checkout_bytes,
        "build_bytes": plan.build_bytes,
        "artifacts_bytes": plan.artifacts_bytes,
        "headroom_bytes": plan.config.caps.disk_headroom_bytes,
        "free_bytes": null,
    });
    match snapshot.disk_free_bytes {
        Known::Unknown => check(
            "PRE-02",
            CheckStatus::Warn,
            format!("free disk is unknown; the round needs {}", gib(need)),
            measured,
        ),
        Known::Known(free) => {
            measured["free_bytes"] = json!(free);
            if need > free {
                check(
                    "PRE-02",
                    CheckStatus::Fail,
                    format!(
                        "the round needs {} of disk; {} is free",
                        gib(need),
                        gib(free)
                    ),
                    measured,
                )
            } else {
                check(
                    "PRE-02",
                    CheckStatus::Pass,
                    format!("{} needed, {} free", gib(need), gib(free)),
                    measured,
                )
            }
        }
    }
}

// ─── PRE-03 memory ──────────────────────────────────────────────────────────

/// Resident memory of one candidate on `harness`.
pub fn footprint_bytes(harness: HarnessKind, local_model_bytes: u64) -> u64 {
    harness.capabilities().footprint(local_model_bytes)
}

fn pre_03_memory(plan: &PreflightPlan, snapshot: &MachineSnapshot, safe_n: u32) -> CheckResult {
    let first_wave: u64 = plan
        .candidates
        .iter()
        .take(safe_n as usize)
        .map(|c| footprint_bytes(c.harness, plan.local_model_bytes))
        .fold(0, u64::saturating_add);
    let mut measured = json!({
        "first_wave": safe_n.min(plan.candidates.len() as u32),
        "first_wave_bytes": first_wave,
        "available_bytes": null,
        "harness_footprint_bytes": HARNESS_FOOTPRINT_BYTES,
        "local_model_bytes": plan.local_model_bytes,
    });
    match snapshot.mem_available_bytes {
        Known::Unknown => check(
            "PRE-03",
            CheckStatus::Warn,
            format!(
                "available memory is unknown; the first wave needs {}",
                gib(first_wave)
            ),
            measured,
        ),
        Known::Known(avail) => {
            measured["available_bytes"] = json!(avail);
            if first_wave > avail {
                check(
                    "PRE-03",
                    CheckStatus::Fail,
                    format!(
                        "the first wave needs {} of memory; {} is available",
                        gib(first_wave),
                        gib(avail)
                    ),
                    measured,
                )
            } else {
                check(
                    "PRE-03",
                    CheckStatus::Pass,
                    format!("first wave {} of {} available", gib(first_wave), gib(avail)),
                    measured,
                )
            }
        }
    }
}

// ─── PRE-04, PRE-05, PRE-08: the parallelism bounds ─────────────────────────

/// Every upper bound on concurrent candidates; `None` is "no bound known".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bounds {
    max_parallel: Option<u32>,
    cpu: Option<u32>,
    memory: Option<u32>,
    fd: Option<u32>,
    process: Option<u32>,
    local_inference: Option<u32>,
    local_candidates: u32,
}

impl Bounds {
    fn of(plan: &PreflightPlan, snapshot: &MachineSnapshot) -> Self {
        let cpu = known(snapshot.cpus).map(|c| c / CPUS_PER_CANDIDATE);
        let memory = known(snapshot.mem_available_bytes).map(|avail| {
            // How many candidates, taken in plan order, fit in memory.
            let mut used = 0u64;
            let mut fit = 0u32;
            for c in &plan.candidates {
                used = used.saturating_add(footprint_bytes(c.harness, plan.local_model_bytes));
                if used > avail {
                    break;
                }
                fit += 1;
            }
            fit
        });
        let fd = known(snapshot.max_open_files).map(|l| clamp_u32(l / FDS_PER_CANDIDATE));
        let process = known(snapshot.max_processes).map(|l| clamp_u32(l / PROCS_PER_CANDIDATE));
        let local_candidates = plan
            .candidates
            .iter()
            .filter(|c| c.harness == HarnessKind::Pi)
            .count() as u32;
        let local_inference = (local_candidates > 0).then(|| local_inference_bound(plan, snapshot));
        Bounds {
            max_parallel: plan.config.caps.max_parallel,
            cpu,
            memory,
            fd,
            process,
            local_inference,
            local_candidates,
        }
    }

    /// `min(n, every known bound)`, at least 1.
    fn safe_n(&self, n: u32) -> u32 {
        [
            Some(n),
            self.max_parallel,
            self.cpu,
            self.memory,
            self.fd,
            self.process,
            self.local_inference,
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(1)
        .max(1)
    }

    fn json(&self) -> serde_json::Value {
        json!({
            "max_parallel": self.max_parallel,
            "cpu": self.cpu,
            "memory": self.memory,
            "fd": self.fd,
            "process": self.process,
            "local_inference": self.local_inference,
        })
    }
}

/// How many local-inference candidates can run at once. Without a GPU that
/// can hold more than one model, local models run one at a time. On Apple
/// Silicon the models share unified memory; on Nvidia each GPU holds one.
fn local_inference_bound(plan: &PreflightPlan, snapshot: &MachineSnapshot) -> u32 {
    match snapshot.gpu {
        GpuClass::AppleSilicon => {
            match (known(snapshot.mem_available_bytes), plan.local_model_bytes) {
                (Some(avail), model) if model > 0 => clamp_u32(avail / model).max(1),
                _ => 1,
            }
        }
        GpuClass::Nvidia { count } => count.max(1),
        GpuClass::None | GpuClass::Unknown => 1,
    }
}

fn known<T: Copy>(value: Known<T>) -> Option<T> {
    match value {
        Known::Known(v) => Some(v),
        Known::Unknown => None,
    }
}

fn clamp_u32(v: u64) -> u32 {
    v.min(u64::from(u32::MAX)) as u32
}

fn pre_04_cpu_gpu(
    plan: &PreflightPlan,
    snapshot: &MachineSnapshot,
    bounds: &Bounds,
) -> CheckResult {
    let measured = json!({
        "cpus": snapshot.cpus,
        "cpu_bound": bounds.cpu,
        "cpus_per_candidate": CPUS_PER_CANDIDATE,
        "gpu": snapshot.gpu,
        "local_candidates": bounds.local_candidates,
        "local_inference_bound": bounds.local_inference,
        "local_model_bytes": plan.local_model_bytes,
    });
    if bounds.cpu.is_none() {
        return check(
            "PRE-04",
            CheckStatus::Warn,
            "the CPU count is unknown",
            measured,
        );
    }
    match bounds.local_inference {
        Some(bound) if matches!(snapshot.gpu, GpuClass::None | GpuClass::Unknown) => check(
            "PRE-04",
            CheckStatus::Warn,
            format!(
                "{} local-inference candidates run on the CPU, {bound} at a time",
                bounds.local_candidates
            ),
            measured,
        ),
        Some(bound) => check(
            "PRE-04",
            CheckStatus::Pass,
            format!(
                "{} local-inference candidates, {bound} at a time",
                bounds.local_candidates
            ),
            measured,
        ),
        None => check("PRE-04", CheckStatus::Pass, "no local inference", measured),
    }
}

fn pre_05_limits(snapshot: &MachineSnapshot, bounds: &Bounds, n: u32) -> CheckResult {
    // The parallelism the operator asked for, before the rlimits cut it.
    let requested = [Some(n), bounds.max_parallel, bounds.cpu, bounds.memory]
        .into_iter()
        .flatten()
        .min();
    let measured = json!({
        "max_open_files": snapshot.max_open_files,
        "max_processes": snapshot.max_processes,
        "fds_per_candidate": FDS_PER_CANDIDATE,
        "procs_per_candidate": PROCS_PER_CANDIDATE,
        "fd_bound": bounds.fd,
        "process_bound": bounds.process,
    });
    let mut short = Vec::new();
    let mut cut = Vec::new();
    for (name, bound) in [("open files", bounds.fd), ("processes", bounds.process)] {
        match bound {
            None => short.push(format!("the {name} limit is unknown")),
            Some(0) => {
                return check(
                    "PRE-05",
                    CheckStatus::Fail,
                    format!("the {name} limit is too low for 1 candidate"),
                    measured,
                )
            }
            Some(b) if requested.is_some_and(|r| b < r) => {
                cut.push(format!("the {name} limit allows {b} candidates at once"))
            }
            Some(_) => {}
        }
    }
    short.extend(cut);
    if short.is_empty() {
        check(
            "PRE-05",
            CheckStatus::Pass,
            "process and open-file limits suffice",
            measured,
        )
    } else {
        check("PRE-05", CheckStatus::Warn, short.join("; "), measured)
    }
}

fn pre_08_safe_n(n: u32, safe_n: u32, waves: u32, bounds: &Bounds) -> CheckResult {
    let mut measured = bounds.json();
    measured["candidates"] = json!(n);
    measured["safe_n"] = json!(safe_n);
    measured["waves"] = json!(waves);
    if n == 0 {
        check(
            "PRE-08",
            CheckStatus::Fail,
            "the plan has no candidates",
            measured,
        )
    } else if safe_n < n {
        check(
            "PRE-08",
            CheckStatus::Warn,
            format!("{n} candidates run {safe_n} at a time in {waves} waves"),
            measured,
        )
    } else {
        check(
            "PRE-08",
            CheckStatus::Pass,
            format!("all {n} candidates run at once"),
            measured,
        )
    }
}

// ─── PRE-06, PRE-07, PRE-12: harness resolution ─────────────────────────────

/// Candidate harnesses without a resolved version.
fn unresolved_harnesses(plan: &PreflightPlan) -> Vec<&'static str> {
    let mut missing: Vec<&'static str> = plan
        .candidates
        .iter()
        .map(|c| c.harness)
        .filter(|h| plan.version_of(*h).is_none())
        .map(HarnessKind::as_str)
        .collect();
    missing.sort_unstable();
    missing.dedup();
    missing
}

fn harness_check(id: &str, plan: &PreflightPlan, ok: &str) -> CheckResult {
    let missing = unresolved_harnesses(plan);
    let measured = json!({ "harness_versions": plan.harness_versions, "unresolved": missing });
    if missing.is_empty() {
        check(id, CheckStatus::Pass, ok, measured)
    } else {
        check(
            id,
            CheckStatus::Fail,
            format!("harness version unresolved: {}", missing.join(", ")),
            measured,
        )
    }
}

fn pre_07_providers(plan: &PreflightPlan) -> CheckResult {
    let mut result = harness_check("PRE-07", plan, "every candidate provider answered");
    result.measured["pools"] = json!(plan.pools);
    if result.status == CheckStatus::Pass {
        let limited: Vec<String> = plan
            .pools
            .iter()
            .filter(|p| p.state != "ok")
            .map(|p| format!("{} is {}", p.pool, p.state))
            .collect();
        if !limited.is_empty() {
            // Design 4.11.1, decision 4: the planner already dropped every
            // candidate whose pool blocks a spawn; this pool-wide state only
            // warns, even when it is `exhausted`.
            result.status = CheckStatus::Warn;
            result.detail = format!("quota: {}", limited.join(", "));
        }
    }
    result
}

// ─── PRE-09 budget ──────────────────────────────────────────────────────────

struct Projection {
    total: MicroUsd,
    unpriced: Vec<String>,
    per_candidate: BTreeMap<String, i64>,
    /// Where each candidate's estimate came from, by label.
    sources: BTreeMap<String, EstimateSource>,
}

/// Expected cost of every candidate at `prices`, summed in n$ and rounded
/// once. Each candidate's tokens come from [`resolve_estimate`] and are
/// priced by [`estimate_cost`], as the live budget prices them.
fn project_cost(plan: &PreflightPlan, prices: &BTreeMap<String, Price>) -> Projection {
    project(
        &plan.candidates,
        &plan.expected_tokens,
        &plan.config.budget.expected_tokens,
        &plan.measured_tokens,
        prices,
    )
}

fn project(
    candidates: &[PreflightCandidate],
    per_label: &BTreeMap<String, TokenEstimate>,
    configured: &ExpectedTokens,
    measured: &BTreeMap<String, MeasuredTokens>,
    prices: &BTreeMap<String, Price>,
) -> Projection {
    let mut total = NanoUsd(0);
    let mut unpriced = Vec::new();
    let mut per_candidate = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for c in candidates {
        let (tokens, source) = resolve_estimate(
            c.model.as_str(),
            per_label.get(&c.label).copied(),
            configured,
            measured,
        );
        sources.insert(c.label.clone(), source);
        match estimate_cost(prices, c.model.as_str(), tokens) {
            Some(cost) => {
                per_candidate.insert(c.label.clone(), cost.to_micro_half_even().0);
                total += cost;
            }
            None => unpriced.push(format!("{} ({})", c.label, c.model)),
        }
    }
    Projection {
        total: total.to_micro_half_even(),
        unpriced,
        per_candidate,
        sources,
    }
}

/// `A: config (sonnet), B: default`: the estimate source of each label.
fn sources_text(sources: &BTreeMap<String, EstimateSource>) -> String {
    sources
        .iter()
        .map(|(label, source)| format!("{label}: {source}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn pre_09_budget(plan: &PreflightPlan, cost: &Projection) -> CheckResult {
    let b = &plan.config.budget;
    let with_judge = cost.total.0.saturating_add(b.judge_reserve_usd_micro);
    let measured = json!({
        "projected_microusd": cost.total.0,
        "judge_reserve_microusd": b.judge_reserve_usd_micro,
        "projected_with_judge_microusd": with_judge,
        "soft_microusd": b.soft_usd_micro,
        "hard_microusd": b.hard_usd_micro,
        "per_candidate_microusd": cost.per_candidate,
        "unpriced": cost.unpriced,
        "estimate_source": cost.sources.iter()
            .map(|(label, source)| (label.clone(), source.to_string()))
            .collect::<BTreeMap<_, _>>(),
    });
    if b.hard_usd_micro <= 0 {
        return check(
            "PRE-09",
            CheckStatus::Fail,
            "no hard budget ceiling is configured (pass --budget-usd)",
            measured,
        );
    }
    let projected = format!(
        "projected {} (estimates: {}) plus judge reserve {}",
        usd(cost.total.0),
        sources_text(&cost.sources),
        usd(b.judge_reserve_usd_micro)
    );
    if with_judge > b.hard_usd_micro {
        check(
            "PRE-09",
            CheckStatus::Fail,
            format!(
                "{projected} exceeds the hard ceiling {}",
                usd(b.hard_usd_micro)
            ),
            measured,
        )
    } else if with_judge > b.soft_usd_micro {
        check(
            "PRE-09",
            CheckStatus::Warn,
            format!(
                "{projected} exceeds the soft limit {}",
                usd(b.soft_usd_micro)
            ),
            measured,
        )
    } else if !cost.unpriced.is_empty() {
        check(
            "PRE-09",
            CheckStatus::Warn,
            format!("{projected}; no price for {}", cost.unpriced.join(", ")),
            measured,
        )
    } else {
        check(
            "PRE-09",
            CheckStatus::Pass,
            format!(
                "{projected} is under the soft limit {}",
                usd(b.soft_usd_micro)
            ),
            measured,
        )
    }
}

// ─── PRE-10, PRE-11, PRE-13 ─────────────────────────────────────────────────

fn pre_10_judge(plan: &PreflightPlan) -> CheckResult {
    let version = plan.version_of(plan.judge_harness);
    let reserve = plan.config.budget.judge_reserve_usd_micro;
    let measured = json!({
        "judge_harness": plan.judge_harness,
        "judge_model": plan.config.judge.model,
        "judge_version": version,
        "judge_reserve_microusd": reserve,
    });
    let mut problems = Vec::new();
    if version.is_none() {
        problems.push(format!(
            "the judge harness {} is not resolved",
            plan.judge_harness.as_str()
        ));
    }
    if reserve <= 0 {
        problems.push("no budget is reserved for the judge".to_string());
    }
    if problems.is_empty() {
        check(
            "PRE-10",
            CheckStatus::Pass,
            format!("judge ready; {} reserved", usd(reserve)),
            measured,
        )
    } else {
        check("PRE-10", CheckStatus::Fail, problems.join("; "), measured)
    }
}

fn pre_11_storage(probe: StorageProbe) -> CheckResult {
    let failed: Vec<&str> = [
        ("write", probe.writable),
        ("lock", probe.lock_ok),
        ("fsync", probe.fsync_ok),
        ("rename", probe.rename_ok),
    ]
    .into_iter()
    .filter(|(_, ok)| !ok)
    .map(|(name, _)| name)
    .collect();
    let measured = json!(probe);
    if failed.is_empty() {
        check(
            "PRE-11",
            CheckStatus::Pass,
            "storage: write, lock, fsync and rename work",
            measured,
        )
    } else {
        check(
            "PRE-11",
            CheckStatus::Fail,
            format!("storage probe failed: {}", failed.join(", ")),
            measured,
        )
    }
}

/// Probe `dir` the way the dataset store uses it: create a private file,
/// write and fsync it, rename it, delete it, and take and release a
/// `DirLock`. Each step that fails sets its flag to `false`; the probe
/// never errors. It leaves nothing behind.
pub fn storage_probe(dir: &Path) -> StorageProbe {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let first = dir.join(format!(".preflight-{}.tmp", &nonce[..12]));
    let second = dir.join(format!(".preflight-{}.done", &nonce[..12]));
    let mut probe = StorageProbe::default();

    match create_private(&first) {
        Ok(mut file) => {
            use std::io::Write;
            probe.writable = file.write_all(b"preflight\n").is_ok();
            probe.fsync_ok = probe.writable && file.sync_all().is_ok();
            drop(file);
            probe.rename_ok = std::fs::rename(&first, &second).is_ok();
            let _ = std::fs::remove_file(if probe.rename_ok { &second } else { &first });
        }
        Err(_) => probe.writable = false,
    }
    probe.lock_ok = DirLock::acquire(
        dir,
        &format!(".preflight-{}", &nonce[..12]),
        Duration::from_secs(60),
        Duration::from_secs(2),
    )
    .map(|guard| guard.release())
    .is_ok();
    probe
}

fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(crate::fsx::PRIVATE_FILE);
    }
    options.open(path)
}

fn pre_13_herdr(plan: &PreflightPlan) -> CheckResult {
    let measured = json!({
        "herdr_reachable": plan.herdr_reachable,
        "horch_exe": plan.horch_exe,
    });
    let mut problems = Vec::new();
    if !plan.herdr_reachable {
        problems.push("herdr is not reachable");
    }
    if plan.horch_exe.is_none() {
        problems.push("the horch executable was not found");
    }
    if problems.is_empty() {
        check(
            "PRE-13",
            CheckStatus::Pass,
            "herdr reachable; horch found",
            measured,
        )
    } else {
        check("PRE-13", CheckStatus::Fail, problems.join("; "), measured)
    }
}

// ─── PRE-14 harness trust ───────────────────────────────────────────────────

fn pre_14_trust(plan: &PreflightPlan) -> CheckResult {
    let mut asking: Vec<HarnessKind> = plan
        .candidates
        .iter()
        .map(|c| c.harness)
        .filter(|h| asks_for_trust(*h))
        .collect();
    asking.sort_unstable_by_key(|h| h.as_str());
    asking.dedup();
    let measured = json!({
        "trust_root": plan.trust_root,
        "trust": plan.trust,
    });
    if asking.is_empty() {
        return check(
            "PRE-14",
            CheckStatus::Pass,
            "no candidate harness asks for trust",
            measured,
        );
    }
    let Some(root) = plan.trust_root.as_deref() else {
        return check(
            "PRE-14",
            CheckStatus::Warn,
            "the repository root is unknown, so harness trust was not checked",
            measured,
        );
    };
    let mut untrusted = Vec::new();
    let mut unknown = Vec::new();
    for harness in asking {
        let found = plan.trust.iter().find(|t| t.harness == harness.as_str());
        match found.map(|t| t.state) {
            Some(TrustState::Trusted) => {}
            Some(TrustState::Untrusted) => untrusted.push(format!(
                "{} has not trusted {} ({}). Run once: {}",
                harness.as_str(),
                root.display(),
                found.map(|t| t.reason.as_str()).unwrap_or_default(),
                trust_fix(harness, root)
            )),
            Some(TrustState::Unknown) | None => unknown.push(format!(
                "{} trust is unknown ({}); a trust dialog may stop its panes",
                harness.as_str(),
                found.map(|t| t.reason.as_str()).unwrap_or("not checked")
            )),
        }
    }
    if !untrusted.is_empty() {
        check("PRE-14", CheckStatus::Fail, untrusted.join("; "), measured)
    } else if !unknown.is_empty() {
        check("PRE-14", CheckStatus::Warn, unknown.join("; "), measured)
    } else {
        check(
            "PRE-14",
            CheckStatus::Pass,
            format!("every candidate harness trusts {}", root.display()),
            measured,
        )
    }
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

fn usd(micro: i64) -> String {
    format!("${}.{:06}", micro / 1_000_000, micro % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::competition::budget::UsageMeter;
    use crate::competition::config::{BudgetConfig, Caps, JudgeConfig, Strategy};

    /// Preflight (PRE-09) and the live budget price a candidate the same
    /// way, for every table model, an unknown model, and a price whose
    /// cache-write rate is not a whole n$ per token.
    #[test]
    fn pre_09_projection_matches_the_live_budget() {
        let mut prices = builtin_prices();
        // 1 n$ per input token; the derived 5-minute write is 1.25 n$.
        let edge = Price {
            input: 0.001,
            output: 0.001,
            cache_read: 0.001,
            cache_write_5m: None,
            cache_write_1h: None,
        };
        prices.insert("edge".into(), edge);
        let meter = UsageMeter {
            prices: prices.clone(),
            ..UsageMeter::default()
        };
        // Without a configured or measured estimate, both use the default.
        let mut models: Vec<String> = prices.keys().cloned().collect();
        models.push("gemini-3-1-pro".into());
        for model in models {
            let c = PreflightCandidate {
                label: "A".into(),
                teammate: TeammateName::new("t").unwrap(),
                harness: HarnessKind::Claude,
                model: ModelId::new(model.as_str()).unwrap(),
                effort: None,
            };
            let p = project(
                &[c],
                &BTreeMap::new(),
                &ExpectedTokens::default(),
                &BTreeMap::new(),
                &prices,
            );
            let preflight = p.per_candidate.get("A").copied().unwrap_or(0);
            assert_eq!(preflight, meter.projected(&model).0, "{model}");
            assert_eq!(
                p.unpriced.is_empty(),
                estimate_cost(&prices, &model, DEFAULT_TOKEN_ESTIMATE).is_some(),
                "{model}"
            );
        }
    }

    /// PRE-07 warns for a quota pool that is not `ok`, also an exhausted
    /// one, and the report still passes (design 4.11.1, decision 4).
    #[test]
    fn pre_07_a_pool_that_is_not_ok_warns_and_does_not_refuse() {
        let candidate = PreflightCandidate {
            label: "A".into(),
            teammate: TeammateName::new("t").unwrap(),
            harness: HarnessKind::Claude,
            model: ModelId::new("opus").unwrap(),
            effort: None,
        };
        let mut plan = PreflightPlan {
            config: DatasetConfig {
                candidates: 1,
                strategy: Strategy::Diverse,
                budget: BudgetConfig::default(),
                judge: JudgeConfig::default(),
                baseline: None,
                gates: Vec::new(),
                caps: Caps::default(),
                exclude: Vec::new(),
                promote_to: None,
                worktree_root: None,
                allow_dirty: false,
                prune_branches: false,
                retain_transcripts: false,
            },
            candidates: vec![candidate],
            harness_versions: BTreeMap::from([("claude".to_string(), Some("2.3.0".to_string()))]),
            judge_harness: HarnessKind::Claude,
            git: GitFacts::default(),
            storage_probe: StorageProbe::default(),
            herdr_reachable: true,
            horch_exe: None,
            pools: vec![PoolFacts {
                pool: "anthropic".into(),
                state: "ok".into(),
            }],
            checkout_bytes: 0,
            build_bytes: 0,
            artifacts_bytes: 0,
            local_model_bytes: 0,
            expected_tokens: BTreeMap::new(),
            measured_tokens: BTreeMap::new(),
            trust_root: None,
            trust: Vec::new(),
        };
        assert_eq!(pre_07_providers(&plan).status, CheckStatus::Pass);
        for state in ["tight", "unknown", "cooling", "exhausted", "broken"] {
            plan.pools[0].state = state.into();
            let result = pre_07_providers(&plan);
            assert_eq!(result.status, CheckStatus::Warn, "{state}");
            assert_eq!(result.detail, format!("quota: anthropic is {state}"));
        }
        // An unresolved harness still fails, whatever the pools say.
        plan.harness_versions.insert("claude".into(), None);
        assert_eq!(pre_07_providers(&plan).status, CheckStatus::Fail);
    }
}
