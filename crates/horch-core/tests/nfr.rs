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
    /// The bytes this thread holds: allocated minus freed, for the RSS guard.
    static LIVE: Cell<i64> = const { Cell::new(0) };
}

/// The system allocator, counting each thread's allocations and live bytes.
struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
        let _ = LIVE.try_with(|n| n.set(n.get() + layout.size() as i64));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = LIVE.try_with(|n| n.set(n.get() - layout.size() as i64));
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
        "RUSTDOCFLAGS=\"-D warnings\" cargo doc --no-deps --workspace",
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

/// Run the gate's `no_spec_todo` function, copied out of
/// scripts/phase-gate.sh, in a scratch repository that tracks `files`.
/// `None` when git is not on PATH (unless `HORCH_REQUIRE_GIT=1`).
fn marker_scan(files: &[(&str, &str)]) -> Option<(bool, String)> {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join("git"))
            .find(|p| p.is_absolute() && p.is_file())
    });
    if found.is_none() {
        assert!(
            std::env::var_os("HORCH_REQUIRE_GIT").is_none_or(|v| v != "1"),
            "HORCH_REQUIRE_GIT=1 is set but git is not on PATH"
        );
        return None;
    }
    let gate = std::fs::read_to_string(repo().join("scripts/phase-gate.sh")).unwrap();
    let start = gate
        .find("no_spec_todo() {")
        .expect("no_spec_todo in phase-gate.sh");
    let end = start + gate[start..].find("\n}\n").unwrap() + 3;
    let tmp = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = tmp.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let git = |args: &[&str]| {
        let mut cmd = std::process::Command::new("git");
        horch_marketplace::git::scrub_repo_env(&mut cmd);
        let st = cmd
            .args(args)
            .current_dir(tmp.path())
            .env("GIT_CONFIG_GLOBAL", tmp.path().join(".gitconfig"))
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    };
    git(&["init", "--quiet"]);
    git(&["add", "."]);
    let mut cmd = std::process::Command::new("bash");
    horch_marketplace::git::scrub_repo_env(&mut cmd);
    let out = cmd
        .args(["-c", &format!("{}no_spec_todo", &gate[start..end])])
        .current_dir(tmp.path())
        .env("GIT_CONFIG_GLOBAL", tmp.path().join(".gitconfig"))
        .output()
        .unwrap();
    Some((
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// U-07, NFR-08: the marker scan fails on a Markdown line that starts with
/// a pending placeholder and on the spec-todo token in any tracked file.
#[test]
fn nfr_08_marker_scan_fails_on_each_marker() {
    let pending = concat!("PEND", "ING:");
    let todo = concat!("SPEC-", "TODO");
    let doc = format!("# Spec\n\n{pending} the operator inserts the text here.\n");
    let Some((ok, err)) = marker_scan(&[("docs/a.md", &doc)]) else {
        return;
    };
    assert!(!ok && err.contains("docs/a.md:3:"), "{err}");
    let indented = format!("- item\n  {pending} later\n");
    let (ok, err) = marker_scan(&[("b.md", &indented)]).unwrap();
    assert!(!ok && err.contains("b.md:2:"), "{err}");
    let code = format!("// {todo}: close this\n");
    let (ok, err) = marker_scan(&[("src/lib.rs", &code)]).unwrap();
    assert!(!ok && err.contains("src/lib.rs:1:"), "{err}");
}

/// U-07: prose that names the pending marker inside a line, lower-case
/// "pending", and the token at a line start outside Markdown pass the scan.
#[test]
fn nfr_08_marker_scan_passes_legit_prose() {
    let pending = concat!("PEND", "ING:");
    let doc =
        format!("The gate fails on a `{pending}` placeholder.\nThe rule is pending review.\n");
    let script = format!("{pending} not a doc\n");
    let Some((ok, err)) = marker_scan(&[("docs/a.md", &doc), ("notes.txt", &script)]) else {
        return;
    };
    assert!(ok, "{err}");
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
    let (cold, cold_cpu) = (Instant::now(), thread_cpu());
    c.tick(now).unwrap();
    let (cold, cold_cpu) = (cold.elapsed(), thread_cpu() - cold_cpu);
    let (warm, warm_cpu) = (Instant::now(), thread_cpu());
    c.tick(now).unwrap();
    let (warm, warm_cpu) = (warm.elapsed(), thread_cpu() - warm_cpu);
    eprintln!(
        "cold {cold:?} ({cold_cpu:?} CPU), steady {warm:?} ({warm_cpu:?} CPU), {} events, RSS {:.1} MiB",
        c.store.len(),
        rss_mib()
    );
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
        (c.store.len() as u64, allocs)
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

/// NFR-02 RSS guard for the debug gate: what the collector keeps per stored
/// event. It kept every `Event` (872 B each on the operator's store of
/// 111,840 events, 93 MiB) until W15; it keeps about 200 B now.
#[test]
fn nfr_02_the_store_keeps_little_per_event() {
    let now = horch_core::clock::parse("2026-09-28T18:00:00Z").unwrap();
    // (stored events, bytes the reopened collector holds)
    let held = |bytes_per_file: u64| {
        let w = world(Part::Whole);
        perf_corpus(&w, 5, bytes_per_file);
        let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now).unwrap();
        c.tick(now).unwrap();
        drop(c);
        let before = LIVE.with(Cell::get);
        let c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now).unwrap();
        let held = LIVE.with(Cell::get) - before;
        (c.store.len() as i64, held)
    };
    let (small_events, small) = held(50_000);
    let (big_events, big) = held(500_000);
    let per_event = (big - small) / (big_events - small_events);
    assert!(
        per_event <= 320,
        "the store keeps {per_event} B per event ({small} B at {small_events} events, \
         {big} B at {big_events})"
    );
}

/// This process's resident set size in MiB, from `ps`.
fn rss_mib() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    let kib: f64 = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0.0);
    kib / 1024.0
}

/// This thread's CPU time: other load on the host barely moves it, where it
/// moves the wall clock a lot.
#[cfg(unix)]
fn thread_cpu() -> std::time::Duration {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `t` is a valid out pointer for the call.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    std::time::Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

#[cfg(not(unix))]
fn thread_cpu() -> std::time::Duration {
    std::time::Duration::ZERO
}

/// `HORCH_PERF_TICKS`, default 8.
fn perf_ticks() -> u32 {
    std::env::var("HORCH_PERF_TICKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8)
}

/// NFR-02 on a real store: the collector's RSS and steady tick over a COPY of
/// a state root (`HORCH_PERF_STATE`; ticks write into it) and the real
/// transcripts under `$HOME`. Run: `HORCH_PERF_STATE=<copy> cargo test --release
/// -p horch-core --test nfr -- --ignored --nocapture nfr_02_live_store_rss`.
#[test]
#[ignore]
fn nfr_02_live_store_rss() {
    let Some(state) = std::env::var_os("HORCH_PERF_STATE") else {
        eprintln!("HORCH_PERF_STATE is not set: nothing to measure");
        return;
    };
    let env = horch_core::runtime::ProcessEnv;
    let inherited = horch_core::runtime::Inherited::from_env(&env);
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    let loc = horch_core::usage::Locations::under_home(&home, &inherited);
    let now = horch_core::clock::now();
    let live = || LIVE.with(Cell::get);
    let before = live();
    let mut c = Collector::open_at(Path::new(&state), loc, Probing::Never, now).unwrap();
    let held = live() - before;
    let events = c.store.len().max(1);
    eprintln!(
        "open: {events} events, {:.1} MiB held ({} B per event), RSS {:.1} MiB",
        held as f64 / 1048576.0,
        held / events as i64,
        rss_mib()
    );
    let mut worst = std::time::Duration::ZERO;
    for n in 1..=perf_ticks() {
        let (t, cpu) = (Instant::now(), thread_cpu());
        c.tick(horch_core::clock::now()).unwrap();
        let (took, cpu) = (t.elapsed(), thread_cpu() - cpu);
        if n > 1 {
            worst = worst.max(took);
        }
        eprintln!(
            "tick {n}: {took:?} ({cpu:?} CPU), {:.1} MiB held, RSS {:.1} MiB",
            (live() - before) as f64 / 1048576.0,
            rss_mib()
        );
    }
    eprintln!("steady tick, worst after the first: {worst:?}");
}
