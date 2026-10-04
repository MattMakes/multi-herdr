//! B2: the dataset config loader and the pure preflight evaluation
//! (PRE-01..PRE-05, PRE-08..PRE-11, PRE-13, PRE-14). Everything here is pure except
//! the storage probe, which works in a temp dir.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use horch_core::competition::config::{
    load, parse_usd_micro, BudgetConfig, Caps, DatasetConfig, JudgeConfig, RunFlags, Strategy,
};
use horch_core::competition::preflight::{
    claude_trust, codex_trust, evaluate, footprint_bytes, parse_git_version, storage_probe,
    trust_fix, CheckStatus, GitFacts, HarnessTrust, PreflightCandidate, PreflightPlan,
    PreflightReport, StorageProbe, TokenEstimate, TrustState,
};
use horch_core::harness::capabilities::HARNESS_FOOTPRINT_BYTES;
use horch_core::harness::HarnessKind;
use horch_core::ids::{ModelId, TeammateName};
use horch_core::runtime::machine::{GpuClass, Known, MachineSnapshot};
use horch_core::usage::money::MicroUsd;

const GIB: u64 = 1024 * 1024 * 1024;
const MIB: u64 = 1024 * 1024;

// ─── fixtures ───────────────────────────────────────────────────────────────

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn machine(name: &str) -> MachineSnapshot {
    let text = std::fs::read_to_string(fixture(&format!("machine/{name}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn mac() -> MachineSnapshot {
    machine("mac-m5-128g")
}

fn linux() -> MachineSnapshot {
    machine("linux-nvidia-2gpu")
}

fn candidate(label: &str, harness: HarnessKind, model: &str) -> PreflightCandidate {
    PreflightCandidate {
        label: label.to_string(),
        teammate: TeammateName::new(label).unwrap(),
        harness,
        model: ModelId::new(model).unwrap(),
        effort: None,
    }
}

fn usd(dollars: i64) -> i64 {
    dollars * 1_000_000
}

fn config() -> DatasetConfig {
    DatasetConfig {
        candidates: 3,
        strategy: Strategy::Diverse,
        budget: BudgetConfig {
            soft_usd_micro: usd(40),
            hard_usd_micro: usd(50),
            judge_reserve_usd_micro: usd(5),
        },
        judge: JudgeConfig::default(),
        baseline: None,
        gates: Vec::new(),
        caps: Caps::default(),
        exclude: Vec::new(),
        promote_to: None,
        worktree_root: Some(PathBuf::from("/Users/op/projects/wt")),
        allow_dirty: false,
        prune_branches: false,
        retain_transcripts: false,
    }
}

/// A plan that passes every check on the Mac fixture.
fn plan() -> PreflightPlan {
    let versions = [
        ("claude", "2.3.0"),
        ("codex", "0.98.0"),
        ("opencode", "1.4.2"),
        ("pi", "0.40.1"),
    ];
    PreflightPlan {
        config: config(),
        candidates: vec![
            candidate("opus", HarnessKind::Claude, "opus"),
            candidate("sol", HarnessKind::Codex, "gpt-5.6-sol"),
            candidate("terra", HarnessKind::OpenCode, "gpt-5.6-terra"),
        ],
        harness_versions: versions
            .iter()
            .map(|(h, v)| (h.to_string(), Some(v.to_string())))
            .collect(),
        judge_harness: HarnessKind::Claude,
        git: GitFacts {
            toplevel: Some(PathBuf::from("/Users/op/projects/app")),
            base_sha: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            dirty: false,
            worktree_supported: true,
            namespace_taken: Vec::new(),
            promote_target_problem: None,
            git_version: Some("git version 2.39.5 (Apple Git-154)".to_string()),
        },
        storage_probe: StorageProbe {
            writable: true,
            lock_ok: true,
            fsync_ok: true,
            rename_ok: true,
        },
        herdr_reachable: true,
        horch_exe: Some(PathBuf::from("/Users/op/.local/bin/horch")),
        pools: Vec::new(),
        checkout_bytes: GIB,
        build_bytes: 5 * GIB,
        artifacts_bytes: GIB,
        local_model_bytes: 0,
        expected_tokens: BTreeMap::new(),
        trust_root: Some(PathBuf::from(APP)),
        trust: vec![trusted("claude"), trusted("codex")],
    }
}

/// The main repository root of the base plan.
const APP: &str = "/Users/op/projects/app";

fn trusted(harness: &str) -> HarnessTrust {
    HarnessTrust {
        harness: harness.to_string(),
        state: TrustState::Trusted,
        reason: "fixture".to_string(),
    }
}

fn status(report: &PreflightReport, id: &str) -> CheckStatus {
    report
        .check(id)
        .unwrap_or_else(|| panic!("{id} missing"))
        .status
}

fn detail(report: &PreflightReport, id: &str) -> String {
    report.check(id).unwrap().detail.clone()
}

#[test]
fn preflight_base_plan_passes_every_check_in_order() {
    let report = evaluate(&plan(), &mac());
    let ids: Vec<&str> = report.checks.iter().map(|c| c.id.as_str()).collect();
    let expected: Vec<String> = (1..=14).map(|i| format!("PRE-{i:02}")).collect();
    assert_eq!(ids, expected);
    for c in &report.checks {
        assert_eq!(c.status, CheckStatus::Pass, "{}: {}", c.id, c.detail);
    }
    assert!(report.passed);
    assert_eq!(report.schema_version, "1.0.0");
    assert_eq!(report.machine, mac());
}

#[test]
fn preflight_report_round_trips_and_digest_tracks_the_environment() {
    let p = plan();
    let a = evaluate(&p, &mac());
    assert_eq!(a, evaluate(&p, &mac()), "evaluate is deterministic");
    let json = serde_json::to_string(&a).unwrap();
    let back: PreflightReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, a);
    assert!(json.contains("\"status\":\"pass\""));

    let mut newer = p.clone();
    newer
        .harness_versions
        .insert("claude".into(), Some("2.4.0".into()));
    assert_ne!(
        evaluate(&newer, &mac()).environment_digest,
        a.environment_digest
    );
    assert_ne!(
        evaluate(&p, &linux()).environment_digest,
        a.environment_digest
    );
    // The budget is not part of the environment.
    let mut richer = p.clone();
    richer.config.budget.hard_usd_micro = usd(500);
    assert_eq!(
        evaluate(&richer, &mac()).environment_digest,
        a.environment_digest
    );
}

// ─── PRE-01 git ─────────────────────────────────────────────────────────────

#[test]
fn pre_01_git_root() {
    let mut p = plan();
    p.git.toplevel = None;
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-01"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-01").contains("not inside a git repository"));
    assert!(!r.passed);
}

#[test]
fn pre_01_base_sha() {
    let mut p = plan();
    p.git.base_sha = None;
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-01"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-01").contains("base commit"));
}

#[test]
fn pre_01_dirty_policy() {
    let mut p = plan();
    p.git.dirty = true;
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-01"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-01").contains("--allow-dirty"));

    p.config.allow_dirty = true;
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-01"), CheckStatus::Pass);
    assert!(detail(&r, "PRE-01").contains("dirty tree allowed"));
}

#[test]
fn pre_01_promote_target_problem_fails() {
    let mut p = plan();
    p.git.promote_target_problem =
        Some("the promotion target 'release' is not a local branch".into());
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-01"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-01").contains("'release' is not a local branch"));
    assert!(!r.passed);
}

#[test]
fn pre_01_worktree_support() {
    assert_eq!(
        parse_git_version("git version 2.39.5 (Apple Git-154)"),
        Some((2, 39))
    );
    assert_eq!(parse_git_version("git version 2.17.0"), Some((2, 17)));
    assert_eq!(parse_git_version("2.45.2.windows.1"), Some((2, 45)));
    assert_eq!(parse_git_version("git version"), None);

    let mut p = plan();
    p.git.git_version = Some("git version 2.16.6".into());
    assert_eq!(status(&evaluate(&p, &mac()), "PRE-01"), CheckStatus::Fail);
    p.git.git_version = Some("git version 2.17.0".into());
    assert_eq!(status(&evaluate(&p, &mac()), "PRE-01"), CheckStatus::Pass);
    p.git.git_version = None;
    assert_eq!(status(&evaluate(&p, &mac()), "PRE-01"), CheckStatus::Fail);

    let mut p = plan();
    p.git.worktree_supported = false;
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-01"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-01").contains("worktrees are not supported"));
}

#[test]
fn pre_01_namespace_free() {
    let mut p = plan();
    p.git.namespace_taken = vec!["mh/exp/0a1b2c3d/r0/opus".into()];
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-01"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-01").contains("mh/exp/0a1b2c3d/r0/opus"));
}

// ─── PRE-02 disk ────────────────────────────────────────────────────────────

#[test]
fn pre_02_disk_budget() {
    let p = plan();
    // 3 × (1 + 5) GiB + 1 GiB artifacts + 10 GiB headroom.
    let need = 3 * 6 * GIB + GIB + 10 * GIB;
    let r = evaluate(&p, &mac());
    assert_eq!(r.check("PRE-02").unwrap().measured["need_bytes"], need);

    let mut m = mac();
    m.disk_free_bytes = Known::Known(need);
    assert_eq!(status(&evaluate(&p, &m), "PRE-02"), CheckStatus::Pass);
    m.disk_free_bytes = Known::Known(need - 1);
    let r = evaluate(&p, &m);
    assert_eq!(status(&r, "PRE-02"), CheckStatus::Fail);
    assert!(!r.passed);
    m.disk_free_bytes = Known::Unknown;
    assert_eq!(status(&evaluate(&p, &m), "PRE-02"), CheckStatus::Warn);

    // Every candidate counts, not only the first wave.
    let mut capped = p.clone();
    capped.config.caps.max_parallel = Some(1);
    let r = evaluate(&capped, &mac());
    assert_eq!(r.check("PRE-02").unwrap().measured["need_bytes"], need);
}

// ─── PRE-03 memory ──────────────────────────────────────────────────────────

#[test]
fn pre_03_memory_footprint() {
    assert_eq!(footprint_bytes(HarnessKind::Claude, 8 * GIB), 600 * MIB);
    assert_eq!(
        footprint_bytes(HarnessKind::Pi, 8 * GIB),
        600 * MIB + 8 * GIB
    );

    // Room for 1 harness: memory bounds the wave to 1, which fits.
    let p = plan();
    let mut m = mac();
    m.mem_available_bytes = Known::Known(GIB);
    let r = evaluate(&p, &m);
    assert_eq!(r.safe_n, 1);
    assert_eq!(r.waves, 3);
    assert_eq!(status(&r, "PRE-03"), CheckStatus::Pass);
    assert_eq!(
        r.check("PRE-03").unwrap().measured["first_wave_bytes"],
        HARNESS_FOOTPRINT_BYTES
    );

    // Not even 1 candidate fits.
    m.mem_available_bytes = Known::Known(500 * MIB);
    let r = evaluate(&p, &m);
    assert_eq!(status(&r, "PRE-03"), CheckStatus::Fail);
    assert!(!r.passed);

    // A local model counts against memory.
    let mut local = plan();
    local.candidates = vec![candidate("qwen", HarnessKind::Pi, "ollama/qwen3")];
    local.local_model_bytes = 100 * GIB;
    assert_eq!(
        status(&evaluate(&local, &mac()), "PRE-03"),
        CheckStatus::Fail
    );

    m.mem_available_bytes = Known::Unknown;
    assert_eq!(status(&evaluate(&p, &m), "PRE-03"), CheckStatus::Warn);
}

// ─── PRE-04 CPU and GPU ─────────────────────────────────────────────────────

fn local_plan(n: usize, model_bytes: u64) -> PreflightPlan {
    let mut p = plan();
    p.candidates = (0..n)
        .map(|i| candidate(&format!("pi-{i}"), HarnessKind::Pi, "ollama/qwen3"))
        .collect();
    p.local_model_bytes = model_bytes;
    p
}

#[test]
fn pre_04_local_inference_bound() {
    // Nvidia with 2 GPUs: 2 local models at once.
    let p = local_plan(3, 8 * GIB);
    let r = evaluate(&p, &linux());
    let c = r.check("PRE-04").unwrap();
    assert_eq!(c.status, CheckStatus::Pass);
    assert_eq!(c.measured["local_inference_bound"], 2);
    assert_eq!(r.safe_n, 2);
    assert_eq!(r.waves, 2);

    // No GPU: 1 at a time, with a warning.
    let mut cpu_only = linux();
    cpu_only.gpu = GpuClass::None;
    let r = evaluate(&p, &cpu_only);
    assert_eq!(status(&r, "PRE-04"), CheckStatus::Warn);
    assert_eq!(
        r.check("PRE-04").unwrap().measured["local_inference_bound"],
        1
    );
    assert_eq!(r.safe_n, 1);
    assert_eq!(r.waves, 3);

    // Apple Silicon: as many models as unified memory holds (80 GiB / 20 GiB).
    let p = local_plan(5, 20 * GIB);
    let r = evaluate(&p, &mac());
    assert_eq!(
        r.check("PRE-04").unwrap().measured["local_inference_bound"],
        4
    );
    // Memory with harness overhead (20.6 GiB each) holds 3.
    assert_eq!(r.safe_n, 3);

    // No local candidate: no bound.
    let r = evaluate(&plan(), &mac());
    assert_eq!(
        r.check("PRE-04").unwrap().measured["local_inference_bound"],
        serde_json::Value::Null
    );
    // The CPU bound is cpus / 2.
    assert_eq!(r.check("PRE-04").unwrap().measured["cpu_bound"], 9);
}

// ─── PRE-05 limits ──────────────────────────────────────────────────────────

#[test]
fn pre_05_rlimits() {
    let p = plan();
    assert_eq!(status(&evaluate(&p, &mac()), "PRE-05"), CheckStatus::Pass);

    // 512 open files: 2 candidates at a time instead of 3.
    let mut m = mac();
    m.max_open_files = Known::Known(512);
    let r = evaluate(&p, &m);
    assert_eq!(status(&r, "PRE-05"), CheckStatus::Warn);
    assert_eq!(r.safe_n, 2);
    assert!(r.passed);

    // Below 1 candidate's need: refuse.
    m.max_open_files = Known::Known(255);
    assert_eq!(status(&evaluate(&p, &m), "PRE-05"), CheckStatus::Fail);
    let mut m = mac();
    m.max_processes = Known::Known(63);
    assert_eq!(status(&evaluate(&p, &m), "PRE-05"), CheckStatus::Fail);

    // The Linux fixture does not know its process limit.
    let r = evaluate(&p, &linux());
    assert_eq!(status(&r, "PRE-05"), CheckStatus::Warn);
    assert!(detail(&r, "PRE-05").contains("processes limit is unknown"));
}

// ─── PRE-08 safe N ──────────────────────────────────────────────────────────

#[test]
fn pre_08_safe_n_waves() {
    let r = evaluate(&plan(), &mac());
    assert_eq!((r.safe_n, r.waves), (3, 1));
    assert_eq!(status(&r, "PRE-08"), CheckStatus::Pass);

    let mut p = plan();
    p.candidates = (0..5)
        .map(|i| candidate(&format!("c{i}"), HarnessKind::Claude, "opus"))
        .collect();
    p.config.caps.max_parallel = Some(2);
    let r = evaluate(&p, &mac());
    assert_eq!((r.safe_n, r.waves), (2, 3));
    let c = r.check("PRE-08").unwrap();
    assert_eq!(c.status, CheckStatus::Warn);
    assert_eq!(c.measured["max_parallel"], 2);

    // The CPU bound: 4 cores, 2 per candidate.
    p.config.caps.max_parallel = None;
    let mut m = mac();
    m.cpus = Known::Known(4);
    let r = evaluate(&p, &m);
    assert_eq!((r.safe_n, r.waves), (2, 3));

    // 1 core still runs 1 candidate.
    m.cpus = Known::Known(1);
    assert_eq!(evaluate(&p, &m).safe_n, 1);

    // Nothing known: N at once.
    let unknown = MachineSnapshot {
        os: "unknown".into(),
        arch: "unknown".into(),
        cpus: Known::Unknown,
        mem_total_bytes: Known::Unknown,
        mem_available_bytes: Known::Unknown,
        disk_free_bytes: Known::Unknown,
        disk_total_bytes: Known::Unknown,
        gpu: GpuClass::Unknown,
        max_open_files: Known::Unknown,
        max_processes: Known::Unknown,
    };
    assert_eq!(evaluate(&p, &unknown).safe_n, 5);

    let mut empty = plan();
    empty.candidates.clear();
    let r = evaluate(&empty, &mac());
    assert_eq!((r.safe_n, r.waves), (1, 0));
    assert_eq!(status(&r, "PRE-08"), CheckStatus::Fail);
}

// ─── PRE-09 budget ──────────────────────────────────────────────────────────

#[test]
fn pre_09_budget_projection_soft_limit() {
    // Default estimate (200k input, 3M cache read, 60k output) at built-in
    // prices: opus $2.60, sol $3.20, terra $1.72.
    let mut p = plan();
    p.config.budget = BudgetConfig {
        soft_usd_micro: usd(10),
        hard_usd_micro: usd(20),
        judge_reserve_usd_micro: usd(2),
    };
    let r = evaluate(&p, &mac());
    assert_eq!(r.projected_cost_microusd, MicroUsd(7_520_000));
    assert_eq!(status(&r, "PRE-09"), CheckStatus::Pass);

    // Projection + judge reserve ($9.52) above the soft limit: warn.
    p.config.budget.soft_usd_micro = usd(9);
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-09"), CheckStatus::Warn);
    assert!(r.passed);

    // Above the hard ceiling: refuse.
    p.config.budget.hard_usd_micro = 9_500_000;
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-09"), CheckStatus::Fail);
    assert!(!r.passed);

    // No hard ceiling: refuse.
    p.config.budget = BudgetConfig::default();
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-09"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-09").contains("no hard budget ceiling"));

    // Per-label estimates replace the default.
    let mut p = plan();
    p.candidates = vec![candidate("opus", HarnessKind::Claude, "opus")];
    p.expected_tokens.insert(
        "opus".into(),
        TokenEstimate {
            input: 1_000_000,
            cache_read: 0,
            output: 0,
        },
    );
    assert_eq!(
        evaluate(&p, &mac()).projected_cost_microusd,
        MicroUsd(4_000_000)
    );

    // The sum is taken in n$ and rounded once: 3 × 500 n$ is 2 µ$, while
    // rounding each 500 n$ (half-even) would give 0.
    let mut p = plan();
    p.candidates = (0..3)
        .map(|i| candidate(&format!("h{i}"), HarnessKind::Claude, "haiku"))
        .collect();
    for c in &p.candidates {
        p.expected_tokens.insert(
            c.label.clone(),
            TokenEstimate {
                input: 0,
                cache_read: 5,
                output: 0,
            },
        );
    }
    assert_eq!(evaluate(&p, &mac()).projected_cost_microusd, MicroUsd(2));

    // A model without a price is left out and named.
    let mut p = plan();
    p.candidates
        .push(candidate("mystery", HarnessKind::Codex, "mystery-model"));
    let r = evaluate(&p, &mac());
    assert_eq!(r.projected_cost_microusd, MicroUsd(7_520_000));
    assert_eq!(status(&r, "PRE-09"), CheckStatus::Warn);
    assert!(detail(&r, "PRE-09").contains("mystery"));
}

// ─── PRE-10 judge ───────────────────────────────────────────────────────────

#[test]
fn pre_10_judge_available_and_reserved() {
    let mut p = plan();
    p.harness_versions.insert("claude".into(), None);
    p.candidates.retain(|c| c.harness != HarnessKind::Claude);
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-10"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-10").contains("judge harness claude"));
    // The candidates themselves resolve.
    assert_eq!(status(&r, "PRE-06"), CheckStatus::Pass);

    let mut p = plan();
    p.config.budget.judge_reserve_usd_micro = 0;
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-10"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-10").contains("reserved"));

    let r = evaluate(&plan(), &mac());
    assert_eq!(status(&r, "PRE-10"), CheckStatus::Pass);
    assert_eq!(
        r.check("PRE-10").unwrap().measured["judge_reserve_microusd"],
        usd(5)
    );
}

// ─── PRE-11 storage ─────────────────────────────────────────────────────────

#[test]
fn pre_11_storage_probe() {
    let tmp = tempfile::tempdir().unwrap();
    let probe = storage_probe(tmp.path());
    assert_eq!(
        probe,
        StorageProbe {
            writable: true,
            lock_ok: true,
            fsync_ok: true,
            rename_ok: true,
        }
    );
    assert_eq!(
        std::fs::read_dir(tmp.path()).unwrap().count(),
        0,
        "the probe leaves nothing behind"
    );

    let mut p = plan();
    p.storage_probe.fsync_ok = false;
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-11"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-11").contains("fsync"));

    let missing = storage_probe(&tmp.path().join("no/such/dir"));
    assert!(!missing.writable);
    assert!(!missing.rename_ok);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let ro = tmp.path().join("ro");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        // root ignores directory permissions; the check means nothing there.
        let root = std::fs::write(ro.join("x"), b"x").is_ok();
        if !root {
            let probe = storage_probe(&ro);
            assert!(!probe.writable);
            assert!(!probe.lock_ok);
            assert!(!probe.rename_ok);
        }
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

// ─── PRE-13 herdr and horch ─────────────────────────────────────────────────

#[test]
fn pre_13_herdr_and_horch_exe() {
    let mut p = plan();
    p.herdr_reachable = false;
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-13"), CheckStatus::Fail);
    assert!(detail(&r, "PRE-13").contains("herdr"));

    let mut p = plan();
    p.horch_exe = None;
    assert_eq!(status(&evaluate(&p, &mac()), "PRE-13"), CheckStatus::Fail);

    // The worktree root no longer matters: trust is PRE-14's question.
    let mut p = plan();
    p.config.worktree_root = Some(PathBuf::from("/tmp/wt"));
    assert_eq!(status(&evaluate(&p, &mac()), "PRE-13"), CheckStatus::Pass);
}

// ─── PRE-14 harness trust ───────────────────────────────────────────────────

fn roots() -> Vec<PathBuf> {
    vec![PathBuf::from(APP)]
}

#[test]
fn pre_14_trusted_root_passes() {
    let r = evaluate(&plan(), &mac());
    assert_eq!(status(&r, "PRE-14"), CheckStatus::Pass);
    assert!(detail(&r, "PRE-14").contains(APP));
}

#[test]
fn pre_14_untrusted_root_refuses_with_the_fix() {
    let mut p = plan();
    p.trust[1].state = TrustState::Untrusted;
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-14"), CheckStatus::Fail);
    assert!(!r.passed);
    let d = detail(&r, "PRE-14");
    assert!(
        d.contains("codex has not trusted /Users/op/projects/app"),
        "{d}"
    );
    assert!(
        d.contains("cd '/Users/op/projects/app' && codex  (choose \"Trust and continue\""),
        "{d}"
    );
    assert!(!d.contains("claude has not"), "{d}");

    // A harness that asks for trust but was not checked only warns.
    let mut p = plan();
    p.trust.clear();
    let r = evaluate(&p, &mac());
    assert_eq!(status(&r, "PRE-14"), CheckStatus::Warn);
    assert!(r.passed);

    // No candidate harness that asks for trust: pass, whatever the stores say.
    let mut p = plan();
    p.candidates.retain(|c| c.harness == HarnessKind::OpenCode);
    p.trust.clear();
    assert_eq!(status(&evaluate(&p, &mac()), "PRE-14"), CheckStatus::Pass);
}

#[test]
fn pre_14_claude_store() {
    let yes = r#"{"oauthAccount":{"x":"secret-token"},"projects":{"/Users/op/projects/app":{"hasTrustDialogAccepted":true}}}"#;
    let t = claude_trust(Some(yes), &roots());
    assert_eq!(t.state, TrustState::Trusted);
    assert!(!t.reason.contains("secret"), "{}", t.reason);

    // A parent folder's entry does not trust the repository.
    let parent = r#"{"projects":{"/Users/op/projects":{"hasTrustDialogAccepted":true},
        "/Users/op/projects/app":{"hasTrustDialogAccepted":false}}}"#;
    assert_eq!(
        claude_trust(Some(parent), &roots()).state,
        TrustState::Untrusted
    );
    let none = r#"{"projects":{}}"#;
    assert_eq!(
        claude_trust(Some(none), &roots()).state,
        TrustState::Untrusted
    );

    // The canonical form of the root also matches (/tmp is /private/tmp).
    let canonical = r#"{"projects":{"/private/tmp/app":{"hasTrustDialogAccepted":true}}}"#;
    let both = [PathBuf::from("/tmp/app"), PathBuf::from("/private/tmp/app")];
    assert_eq!(
        claude_trust(Some(canonical), &both).state,
        TrustState::Trusted
    );

    // A missing file is untrusted; a broken one is unknown.
    let missing = claude_trust(None, &roots());
    assert_eq!(missing.state, TrustState::Untrusted);
    assert!(missing.reason.contains("does not exist"));
    assert_eq!(
        claude_trust(Some("{not json"), &roots()).state,
        TrustState::Unknown
    );
}

#[test]
fn pre_14_codex_store() {
    let yes = "model = \"gpt-5\"\n\n[projects.\"/Users/op/projects/app\"]\ntrust_level = \"trusted\"\n\n[projects.\"/other\"]\ntrust_level = \"untrusted\"\n";
    assert_eq!(codex_trust(Some(yes), &roots()).state, TrustState::Trusted);
    let literal = "[projects.'/Users/op/projects/app']\ntrust_level = 'trusted'\n";
    assert_eq!(
        codex_trust(Some(literal), &roots()).state,
        TrustState::Trusted
    );

    // trust_level under another table does not count.
    let other = "[projects.\"/Users/op/projects\"]\ntrust_level = \"trusted\"\n[tui]\ntrust_level = \"trusted\"\n";
    assert_eq!(
        codex_trust(Some(other), &roots()).state,
        TrustState::Untrusted
    );
    let untrusted = "[projects.\"/Users/op/projects/app\"]\ntrust_level = \"untrusted\"\n";
    assert_eq!(
        codex_trust(Some(untrusted), &roots()).state,
        TrustState::Untrusted
    );
    assert_eq!(codex_trust(Some(""), &roots()).state, TrustState::Untrusted);
    let missing = codex_trust(None, &roots());
    assert_eq!(missing.state, TrustState::Untrusted);
    assert!(missing.reason.contains("does not exist"));
}

#[test]
fn pre_14_fix_commands_quote_the_root() {
    let root = Path::new("/Users/op/it's here");
    assert_eq!(
        trust_fix(HarnessKind::Claude, root),
        "cd '/Users/op/it'\\''s here' && claude  (choose \"Yes, I trust this folder\", then exit)"
    );
    assert!(trust_fix(HarnessKind::Antigravity, root).contains("&& agy"));
}

// ─── config ─────────────────────────────────────────────────────────────────

fn project_with(yaml: Option<&Path>) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    if let Some(src) = yaml {
        std::fs::create_dir_all(tmp.path().join(".multi-herdr")).unwrap();
        std::fs::copy(src, tmp.path().join(".multi-herdr/dataset.yaml")).unwrap();
    }
    tmp
}

fn flags() -> RunFlags {
    RunFlags {
        task: "add a --json flag".into(),
        ..RunFlags::default()
    }
}

#[test]
fn config_flags_override_file() {
    let project = project_with(Some(&fixture("dataset/full.yaml")));

    let file_only = load(project.path(), &flags()).unwrap();
    assert_eq!(file_only.candidates, 4);
    assert_eq!(file_only.budget.hard_usd_micro, usd(10));
    assert_eq!(
        file_only.baseline,
        Some(TeammateName::new("sonnet").unwrap())
    );
    assert_eq!(file_only.gates.len(), 2);
    assert!(file_only.gates[0].required, "required defaults to true");
    assert!(!file_only.gates[1].required);
    assert_eq!(file_only.judge.timeout_s, 600);
    assert_eq!(file_only.judge.model, "opus");
    assert_eq!(file_only.caps.max_parallel, Some(3));
    assert_eq!(file_only.caps.candidate_deadline_s, 1800);
    // Caps the file leaves out keep their defaults.
    assert_eq!(file_only.caps.log_cap_bytes, 262_144);
    assert_eq!(file_only.caps.output_cap_bytes, 1_048_576);
    assert_eq!(file_only.exclude, vec![TeammateName::new("smoke").unwrap()]);

    let over = RunFlags {
        candidates: Some(2),
        budget_usd: Some("20".into()),
        baseline: Some(TeammateName::new("opus").unwrap()),
        promote_to: Some("main".into()),
        worktree_root: Some(PathBuf::from("/srv/wt")),
        allow_dirty: true,
        ..flags()
    };
    let c = load(project.path(), &over).unwrap();
    assert_eq!(c.candidates, 2);
    assert_eq!(c.budget.hard_usd_micro, usd(20));
    // The file sets the soft limit and reserve; the flag does not move them.
    assert_eq!(c.budget.soft_usd_micro, usd(8));
    assert_eq!(c.budget.judge_reserve_usd_micro, usd(1));
    assert_eq!(c.baseline, Some(TeammateName::new("opus").unwrap()));
    assert_eq!(c.promote_to.as_deref(), Some("main"));
    assert_eq!(c.worktree_root, Some(PathBuf::from("/srv/wt")));
    assert!(c.allow_dirty);
    assert_eq!(c.gates, file_only.gates);

    // No file: defaults.
    let bare = project_with(None);
    let c = load(bare.path(), &flags()).unwrap();
    assert_eq!(c.candidates, 3);
    assert_eq!(c.strategy, Strategy::Diverse);
    assert_eq!(c.budget, BudgetConfig::default());
    assert_eq!(c.caps, Caps::default());
    assert!(!c.retain_transcripts);
}

#[test]
fn config_rejects_unknown_key() {
    let project = project_with(Some(&fixture("dataset/unknown-key.yaml")));
    let err = format!("{:#}", load(project.path(), &flags()).unwrap_err());
    assert!(err.contains("unknown field `candidate`"), "{err}");

    // Unknown keys inside a section too.
    let project = project_with(None);
    let file = project.path().join(".multi-herdr/dataset.yaml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "caps:\n  max_paralel: 2\n").unwrap();
    assert!(load(project.path(), &flags()).is_err());

    // Invalid values.
    for (yaml, why) in [
        ("candidates: 0\n", "candidates must be at least 1"),
        (
            "budget:\n  soft_usd_micro: 9\n  hard_usd_micro: 5\n",
            "below the soft limit",
        ),
        (
            "budget:\n  soft_usd_micro: 4\n  hard_usd_micro: 5\n  judge_reserve_usd_micro: 5\n",
            "judge reserve",
        ),
        (
            "gates:\n  - name: t\n    command: ' '\n    timeout_s: 5\n",
            "empty command",
        ),
        (
            "gates:\n  - name: t\n    command: make\n    timeout_s: 0\n",
            "timeout_s",
        ),
    ] {
        std::fs::write(&file, yaml).unwrap();
        let err = format!("{:#}", load(project.path(), &flags()).unwrap_err());
        assert!(err.contains(why), "{yaml}: {err}");
    }
}

#[test]
fn config_budget_decimal_exact() {
    let project = project_with(None);
    let c = load(
        project.path(),
        &RunFlags {
            budget_usd: Some("12.345678".into()),
            ..flags()
        },
    )
    .unwrap();
    assert_eq!(c.budget.hard_usd_micro, 12_345_678);
    // 80 % of the ceiling, rounded down; the judge reserve is 10 %.
    assert_eq!(c.budget.soft_usd_micro, 9_876_542);
    assert_eq!(c.budget.judge_reserve_usd_micro, 1_234_567);

    for (text, micro) in [
        ("0.1", 100_000),
        ("1", 1_000_000),
        ("1.", 1_000_000),
        (".5", 500_000),
        ("0.000001", 1),
        ("3.1000000", 3_100_000),
        ("100000.000000", 100_000_000_000),
    ] {
        assert_eq!(parse_usd_micro(text).unwrap(), micro, "{text}");
    }
    for bad in [
        "",
        ".",
        "-1",
        "1.0000001",
        "1e3",
        "$5",
        "1,5",
        "99999999999999999999",
    ] {
        assert!(parse_usd_micro(bad).is_err(), "{bad:?}");
    }
}
