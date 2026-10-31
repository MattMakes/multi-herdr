//! `assess_pool` keeps its state, reason, worst window and headroom for every
//! reading in the quota fixtures. The golden file was written by the code
//! before the #153 refactor. The test only reads it: a test that rewrote the
//! file could not fail.

use std::fmt::Write;
use std::path::Path;

use horch_core::routing::policy::Policy;
use horch_core::routing::quota::{
    PoolReading, QuotaFile, QuotaView, Window, POOL_CLAUDE, POOL_CODEX, POOL_GOOGLE, POOL_ZEN,
};

const POOLS: [&str; 6] = [
    POOL_CLAUDE,
    POOL_CODEX,
    POOL_GOOGLE,
    POOL_ZEN,
    "local",
    "unknown",
];
const SCOPES: [Option<&str>; 3] = [None, Some("fable"), Some("opus")];
const LOCAL_MODELS: [Option<&str>; 3] = [None, Some("qwen3.8"), Some("missing-model")];
/// Before and after the zen cooldown of `opencode-cooling.json`.
const NOWS: [&str; 3] = [
    "2026-09-28T18:00:00Z",
    "2026-09-28T19:00:00Z",
    "2026-10-30T00:00:00Z",
];

fn dump() -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/telemetry/quota");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    let mut out = String::new();
    for name in &names {
        for now in NOWS {
            for from_override in [true, false] {
                let view = QuotaView::new(
                    QuotaFile::read(&dir.join(name)).unwrap(),
                    horch_core::clock::parse(now).unwrap(),
                    Policy::default(),
                    from_override,
                );
                for pool in POOLS {
                    for scope in SCOPES {
                        for local in LOCAL_MODELS {
                            let a = view.assess_pool(pool, scope, local);
                            writeln!(
                                out,
                                "{name} {now} {from_override} {pool} {scope:?} {local:?} => {} | {} | {:?} | {:?}",
                                a.state,
                                a.reason,
                                a.headroom_per_h,
                                a.worst
                                    .map(|w| format!("{} {:?} {:?}", w.label(), w.used, w.resets_at)),
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn assess_pool_matches_the_golden_file() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/r_routing_assess.txt");
    let got = dump();
    let want = std::fs::read_to_string(&golden).unwrap();
    assert!(
        want == got,
        "assess_pool output changed; see {}",
        golden.display()
    );
}

fn view_of(pool: &str, reading: PoolReading) -> QuotaView {
    let mut file = QuotaFile::default();
    file.pools.insert(pool.to_string(), reading);
    QuotaView::new(
        file,
        horch_core::clock::parse(NOWS[0]).unwrap(),
        Policy::default(),
        true,
    )
}

#[test]
fn a_local_model_missing_from_ollama_list_is_broken() {
    let view = view_of(
        "local",
        PoolReading {
            models: Some(vec!["qwen3.8:latest".into()]),
            ..PoolReading::default()
        },
    );
    let missing = view.assess_pool("local", None, Some("llama9"));
    assert_eq!(missing.state.to_string(), "broken");
    assert_eq!(missing.reason, "model llama9 is not in ollama list");
    // `:latest` is optional, and no wanted model skips the check.
    assert_eq!(
        view.assess_pool("local", None, Some("qwen3.8"))
            .state
            .to_string(),
        "ok"
    );
    assert_eq!(
        view.assess_pool("local", None, None).state.to_string(),
        "ok"
    );
}

#[test]
fn a_local_reading_marked_broken_without_an_error_says_broken() {
    let view = view_of(
        "local",
        PoolReading {
            state: "broken".into(),
            ..PoolReading::default()
        },
    );
    let a = view.assess_pool("local", None, None);
    assert_eq!(
        (a.state.to_string().as_str(), a.reason.as_str()),
        ("broken", "broken")
    );
}

#[test]
fn windows_of_another_scope_leave_the_pool_unknown() {
    let view = view_of(
        POOL_CLAUDE,
        PoolReading {
            observed_at: Some(NOWS[0].into()),
            windows: vec![Window {
                name: "7d".into(),
                minutes: 10080,
                scope_model: Some("fable".into()),
                used: 0.1,
                resets_at: None,
            }],
            ..PoolReading::default()
        },
    );
    let a = view.assess_pool(POOL_CLAUDE, Some("opus"), None);
    assert_eq!(a.state.to_string(), "unknown");
    assert_eq!(a.reason, "no matching window for opus");
    let a = view.assess_pool(POOL_CLAUDE, None, None);
    assert_eq!(a.reason, "no matching window for this pool");
}
