//! Non-functional requirements that a test can hold (design section 3, NFR).

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::Path;
use std::time::Instant;

use common::*;
use horch_core::telemetry::collect::{Collector, Probing};

thread_local! {
    /// The allocations this thread has made, for the NFR-02 guard.
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

/// The system allocator, counting each thread's allocations.
struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

/// NFR-03: the repo's roster, fallbacks included, passes `--check`.
#[test]
fn nfr_03_repo_roster_check_passes() {
    let dir = repo().join("teammates");
    let roster =
        horch_core::roster::Roster::load_layered(None, None, Some(&dir.to_string_lossy())).unwrap();
    let problems = roster.check();
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The dependency names in one `[...dependencies]` table of a Cargo.toml.
fn deps(manifest: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(manifest).unwrap();
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line.ends_with("dependencies]")
                && !line.contains("dev-dependencies")
                && !line.contains("build-dependencies");
            continue;
        }
        if in_deps && !line.is_empty() && !line.starts_with('#') {
            if let Some((name, _)) = line.split_once('=') {
                out.push(name.trim().trim_end_matches(".workspace").to_string());
            }
        }
    }
    out
}

/// NFR-05: no new runtime crate beyond crossterm (and ratatui, which the
/// design allowed but the screen does not need), and only in `horch`.
#[test]
fn nfr_05_no_new_runtime_crates() {
    let allowed_core = [
        "anyhow",
        "serde",
        "serde_json",
        "serde_yaml",
        "chrono",
        "uuid",
        "libc",
        "sha2",
        "horch-marketplace",
    ];
    for d in deps(&repo().join("crates/horch-core/Cargo.toml")) {
        assert!(
            allowed_core.contains(&d.as_str()),
            "horch-core gained '{d}'"
        );
    }
    let allowed = [
        "horch-core",
        "anyhow",
        "clap",
        "serde",
        "serde_json",
        "chrono",
        "tempfile",
        "libc",
        "ratatui",
        "crossterm",
    ];
    for d in deps(&repo().join("crates/horch/Cargo.toml")) {
        assert!(allowed.contains(&d.as_str()), "horch gained '{d}'");
    }
}

/// Every dependency name in every table of a Cargo.toml: normal, dev,
/// build, target-specific and `[workspace.dependencies]`.
fn all_deps(manifest: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(manifest).unwrap();
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line.trim_end_matches(']').ends_with("dependencies");
            continue;
        }
        if in_deps && !line.is_empty() && !line.starts_with('#') {
            if let Some((name, _)) = line.split_once('=') {
                out.push(name.trim().trim_end_matches(".workspace").to_string());
            }
        }
    }
    out
}

/// The root Cargo.toml and every `crates/*/Cargo.toml`, with the crate's
/// directory name (`""` for the root).
fn workspace_manifests() -> Vec<(String, std::path::PathBuf)> {
    let mut out = vec![(String::new(), repo().join("Cargo.toml"))];
    for entry in std::fs::read_dir(repo().join("crates")).unwrap().flatten() {
        let manifest = entry.path().join("Cargo.toml");
        if manifest.is_file() {
            out.push((entry.file_name().to_string_lossy().into_owned(), manifest));
        }
    }
    out
}

/// Fails if any workspace Cargo.toml outside `exempt` names one of `banned`.
fn assert_no_workspace_dep(banned: &[&str], exempt: &[&str], why: &str) {
    for (krate, manifest) in workspace_manifests() {
        if exempt.contains(&krate.as_str()) {
            continue;
        }
        for d in all_deps(&manifest) {
            assert!(
                !banned.contains(&d.as_str()),
                "{why}: {} names '{d}'",
                manifest.display()
            );
        }
    }
}

/// NFR-06 (OD6): horch-marketplace stays on its allowed list, and no horch
/// crate takes an HTTP client. herdr-install and herdr-docs-sync are the
/// standalone download tools; they used ureq before this rule.
#[test]
fn nfr_06_dependency_allowlist() {
    let manifest = repo().join("crates/horch-marketplace/Cargo.toml");
    if manifest.is_file() {
        let allowed = ["anyhow", "serde", "serde_json", "serde_yaml", "sha2"];
        for d in deps(&manifest) {
            assert!(
                allowed.contains(&d.as_str()),
                "horch-marketplace gained '{d}'"
            );
        }
    }
    assert_no_workspace_dep(
        &["reqwest", "ureq", "hyper", "isahc", "attohttpc", "curl"],
        &["herdr-install", "herdr-docs-sync"],
        "NFR-06: no HTTP crate",
    );
}

/// NFR-09: everything is synchronous; no async runtime and no database crate.
#[test]
fn nfr_09_no_async_runtime_deps() {
    assert_no_workspace_dep(
        &[
            "tokio",
            "async-std",
            "smol",
            "futures",
            "rusqlite",
            "sqlx",
            "diesel",
        ],
        &[],
        "NFR-09: sync only",
    );
}

/// NFR-11: randomized tests use the in-repo PRNG, not a property-test crate.
#[test]
fn nfr_11_no_proptest() {
    assert_no_workspace_dep(&["proptest", "quickcheck"], &[], "NFR-11: in-repo PRNG");
}

/// NFR-08: `just gate` (scripts/phase-gate.sh) runs every check of the
/// master plan's section 5, in order.
#[test]
fn nfr_08_phase_gate_runs_every_check() {
    let gate = std::fs::read_to_string(repo().join("scripts/phase-gate.sh")).unwrap();
    let mut from = 0;
    for check in [
        "step no_spec_todo",
        "cargo fmt --all --check",
        "cargo build --workspace --all-targets",
        "cargo build --workspace --bins",
        "cargo clippy --workspace --all-targets -- -D warnings",
        "cargo test --workspace",
        "HORCH_TEAMMATES_DIR=teammates",
        "teammates --check",
        "scripts/check-req-coverage.sh",
        "scripts/check-deps.sh",
        "scripts/verify-telemetry-e2e.sh",
        "step godot_skills",
        "GATE GREEN",
    ] {
        let at = gate[from..]
            .find(check)
            .unwrap_or_else(|| panic!("phase-gate.sh lacks '{check}' (in order)"));
        from += at + check.len();
    }
    assert!(gate.contains("set -euo pipefail"));
}

/// The NFR-02 corpus: `sessions` live Claude sessions of about
/// `bytes_per_file` bytes each, and one ledger that names them all.
fn perf_corpus(w: &World, sessions: usize, bytes_per_file: u64) {
    let template =
        std::fs::read_to_string(fixtures().join(format!("claude/-work-alpha/{A2}.jsonl"))).unwrap();
    let assistant: Vec<&str> = template
        .lines()
        .filter(|l| l.contains("\"assistant\""))
        .collect();
    let mut ledger = Vec::new();
    for n in 0..sessions {
        let sid = format!("{n:08}-0000-4000-8000-000000000000");
        let path = w.home.join(format!(".claude/projects/-perf/{sid}.jsonl"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        let mut written = 0u64;
        let mut i = 0u64;
        while written < bytes_per_file {
            let line = assistant[(i % assistant.len() as u64) as usize]
                .replace("msg_d", &format!("msg_{i}_d"))
                .replace("msg_e", &format!("msg_{i}_e"));
            written += line.len() as u64 + 1;
            std::io::Write::write_all(&mut f, line.as_bytes()).unwrap();
            std::io::Write::write_all(&mut f, b"\n").unwrap();
            i += 1;
        }
        ledger.push(serde_json::json!({
            "record_id": format!("perf-{n}"), "session_id": sid, "agent": "claude", "tier": "sonnet",
            "model": "sonnet", "role": format!("sonnet-{n}"), "status": "working", "task": "perf",
            "history": [], "created_at": "2026-09-28T17:00:00Z", "updated_at": "2026-09-28T17:00:00Z"
        }));
    }
    std::fs::write(
        w.state.join("-perf.json"),
        serde_json::to_string(&ledger).unwrap(),
    )
    .unwrap();
}

/// NFR-02: a steady-state tick over 50 live sessions under 200 ms, and a cold
/// start under 30 s. Generates a corpus of `HORCH_PERF_MB` (default 1024) MB.
/// Run with `just verify-perf`.
#[test]
#[ignore]
fn nfr_02_tick_budget() {
    let w = world(Part::Whole);
    let mb: u64 = std::env::var("HORCH_PERF_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1024);
    perf_corpus(&w, 50, mb * 1024 * 1024 / 50);
    let now = horch_core::clock::parse("2026-09-28T18:00:00Z").unwrap();
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now).unwrap();
    let cold = Instant::now();
    c.tick(now).unwrap();
    let cold = cold.elapsed();
    let warm = Instant::now();
    c.tick(now).unwrap();
    let warm = warm.elapsed();
    eprintln!("cold {cold:?}, steady {warm:?}");
    assert!(cold.as_secs() < 30, "cold start {cold:?}");
    assert!(warm.as_millis() < 200, "steady tick {warm:?}");
}

/// NFR-02 guard for the debug gate, without a clock: a steady tick makes no
/// allocation per stored event. `nfr_02_tick_budget` runs in release only.
/// The snapshot once built its 18 rollups (3 windows by 6 groups) with 2
/// `String`s per event each, and the 1 GB steady tick took 1.6 s.
#[test]
fn nfr_02_steady_tick_allocates_nothing_per_event() {
    let now = horch_core::clock::parse("2026-09-28T18:00:00Z").unwrap();
    // (stored events, allocations in the second tick)
    let steady = |bytes_per_file: u64| {
        let w = world(Part::Whole);
        perf_corpus(&w, 5, bytes_per_file);
        let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now).unwrap();
        c.tick(now).unwrap();
        let before = ALLOCS.with(Cell::get);
        c.tick(now).unwrap();
        let allocs = ALLOCS.with(Cell::get) - before;
        (c.store.events.len() as u64, allocs)
    };
    let (small_events, small) = steady(50_000);
    let (big_events, big) = steady(500_000);
    assert!(
        big_events >= 5 * small_events,
        "{small_events} vs {big_events} events"
    );
    let extra = big.saturating_sub(small);
    assert!(
        extra * 100 < big_events - small_events,
        "a steady tick allocates per stored event: {small} allocations at {small_events} \
         events, {big} at {big_events}"
    );
}
