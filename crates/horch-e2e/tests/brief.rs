//! End-to-end: the worker brief carries every binary override to the worker
//! pane, and the worker hands them to its agent through the child's
//! environment only.

use std::path::PathBuf;
use std::process::Output;

use horch_e2e::harness::{fixtures, Harness};
use serde_json::Value;

/// The overrides only the brief can carry: the worker below runs without them.
const CARRIED: [&str; 3] = ["HORCH_OPENCODE_BIN", "HORCH_PI_BIN", "HORCH_SQLITE3_BIN"];

fn text(o: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn arc_07_e2e_bin_overrides_reach_worker() {
    let mut h = Harness::new("arc07");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    // Distinct values, so the brief cannot have picked up a default.
    let fakes: Vec<(&str, PathBuf)> = CARRIED
        .iter()
        .map(|key| (*key, h.bin.join(format!("fake-for-{key}"))))
        .collect();
    for (key, path) in &fakes {
        h.set(key, path.to_string_lossy());
    }

    // Workspace w1 with its root pane w1:p1, as `horch fleet` would leave it.
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let created = create
        .args(["workspace", "create", "--label", "arc07"])
        .output()
        .unwrap();
    assert!(created.status.success(), "{}", text(&created));

    let out = h.run(&["spawn", "sonnet", "x", "--from-pane", "w1:p1", "--no-tile"]);
    assert!(out.status.success(), "{}", text(&out));
    let pane = String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_string();

    // The brief on disk: schema 2, every override, the v1 keys kept.
    let brief_path = h.tmp.join("herdr-orchestration-w1/sonnet-1.brief.json");
    let brief: Value =
        serde_json::from_str(&std::fs::read_to_string(&brief_path).unwrap()).unwrap();
    assert_eq!(brief["schema"], 2);
    for (key, path) in &fakes {
        let field = match *key {
            "HORCH_OPENCODE_BIN" => "opencode",
            "HORCH_PI_BIN" => "pi",
            _ => "sqlite3",
        };
        assert_eq!(
            brief["bin_overrides"][field].as_str(),
            Some(path.to_string_lossy().as_ref()),
            "{key}"
        );
    }
    assert_eq!(
        brief["claude_bin"].as_str(),
        Some(h.bin.join("claude").to_string_lossy().as_ref()),
        "a v1 worker still finds the claude override"
    );

    // The worker side: `horch worker` in the new pane, which (like a real
    // pane) does not inherit the spawner's environment, so the three
    // overrides are absent from it.
    let mut worker = h.horch(&["worker", "sonnet-1"]);
    worker.env("HERDR_PANE_ID", &pane);
    worker.env_remove("HORCH_WORKSPACE_ID");
    for key in CARRIED {
        worker.env_remove(key);
    }
    let out = worker.output().unwrap();
    assert!(out.status.success(), "{}", text(&out));

    // The agent the worker launched received them from the transport env.
    let claude = h
        .calls_of("claude")
        .into_iter()
        .last()
        .expect("the worker launched claude");
    let keys: Vec<String> = serde_json::from_value(claude["env_keys"].clone()).unwrap();
    for key in CARRIED.iter().chain(&[
        "HORCH_ROLE",
        "HORCH_RECORD_ID",
        "HORCH_WORKSPACE_ID",
        "HORCH_CLAUDE_BIN",
    ]) {
        assert!(keys.iter().any(|k| k == key), "{key} not in {keys:?}");
    }
    assert!(!keys.iter().any(|k| k == "ANTHROPIC_API_KEY"), "{keys:?}");
    // fake-claude flags an `ANTHROPIC_API_KEY` in its environment.
    assert_eq!(claude["violations"], serde_json::json!([]), "{claude}");
}
