//! The hermetic end-to-end story of design section 16.5, one step at a time.
//! `scripts/verify-telemetry-e2e.sh` runs it and keeps its `PASS <step>`
//! lines. Every number comes from the fixture oracle, EXPECTED.md.

use std::collections::HashSet;
use std::path::Path;

use horch_e2e::harness::{copy_tree, fixtures, Harness};
use serde_json::Value;

fn pass(step: &str) {
    println!("PASS {step}");
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Copy a tree, keeping the first floor(n/2) lines of every `.jsonl`.
fn copy_half(from: &Path, to: &Path) {
    if from.is_dir() {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            copy_half(&e.path(), &to.join(e.file_name()));
        }
        return;
    }
    let text = std::fs::read_to_string(from).unwrap();
    let text = if from.extension().and_then(|e| e.to_str()) == Some("jsonl") {
        let lines: Vec<&str> = text.lines().collect();
        lines[..lines.len() / 2]
            .iter()
            .map(|l| format!("{l}\n"))
            .collect()
    } else {
        text
    };
    std::fs::write(to, text).unwrap();
}

fn events(h: &Harness) -> Vec<Value> {
    let dir = h.state.join("telemetry");
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with("events-") {
            for line in std::fs::read_to_string(e.path()).unwrap().lines() {
                out.push(serde_json::from_str(line).unwrap());
            }
        }
    }
    out
}

fn total(events: &[Value]) -> f64 {
    events
        .iter()
        .map(|e| e["cost_usd"].as_f64().unwrap_or(0.0))
        .sum()
}

fn no_duplicate_keys(events: &[Value]) {
    let mut keys = HashSet::new();
    for e in events {
        let k = (
            e["agent"].to_string(),
            e["session_id"].to_string(),
            e["event_id"].to_string(),
        );
        assert!(keys.insert(k.clone()), "duplicate event key {k:?}");
    }
}

fn check_golden(name: &str, frame: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var("HORCH_BLESS").ok().as_deref() == Some("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, frame).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; run once with HORCH_BLESS=1 and review",
            path.display()
        )
    });
    assert_eq!(frame, want, "{name}");
}

#[test]
fn the_hermetic_story() {
    let mut h = Harness::new("story");
    let f = fixtures();
    let places = [
        (f.join("claude"), h.home.join(".claude/projects")),
        (f.join("codex/sessions"), h.home.join(".codex/sessions")),
        (f.join("pi/sessions"), h.home.join(".pi/agent/sessions")),
        (f.join("prime"), h.state.join("prime")),
    ];

    // 1. The clock, the 2 ledgers, and the first half of every transcript.
    h.set("HORCH_NOW", "2026-09-28T18:00:00Z");
    h.set("HORCH_PROBE_TIMEOUT_MS", "1500");
    for (from, to) in &places {
        copy_half(from, to);
    }
    for ledger in ["-work-alpha.json", "-work-beta.json"] {
        let text = std::fs::read_to_string(f.join("ledgers").join(ledger)).unwrap();
        std::fs::write(
            h.state.join(ledger),
            text.replace("{STATE}", &h.state.to_string_lossy()),
        )
        .unwrap();
    }
    pass("1 fixtures installed (first halves)");

    // 2. One tick: EXPECTED.md stage 1.
    let out = h.run(&["telemetry", "collect", "--once"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ev = events(&h);
    assert!(close(total(&ev), 0.0783506), "stage 1 total {}", total(&ev));
    let snap: Value = serde_json::from_str(
        &std::fs::read_to_string(h.state.join("telemetry/snapshot.json")).unwrap(),
    )
    .unwrap();
    let week: f64 = snap["rollups"]["7d"]["by_kind"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["cost_usd"].as_f64().unwrap())
        .sum();
    assert!(
        close(week, 0.0783506 - 0.00049),
        "stage 1, 7d window {week}"
    );
    pass("2 stage 1 totals equal EXPECTED.md");

    // 4 (before 3): a collector that dies between the append and the cursor
    // save, as `kill -9` would leave it: events written, cursors not, lock stale.
    for (from, to) in &places {
        copy_tree(from, to);
    }
    h.set("HORCH_NOW", "2026-09-28T18:00:02Z");
    h.set("HORCH_FAULT", "abort-after-append");
    let crashed = h.run(&["telemetry", "collect", "--once"]);
    assert!(!crashed.status.success(), "the fault must stop the tick");
    assert!(
        h.state.join("telemetry/collector.lock").is_dir(),
        "a stale lock is left behind"
    );
    h.unset("HORCH_FAULT");

    // 3. The second halves are in; the next tick reaches stage 2, the stale
    // lock is broken, and nothing is counted twice.
    let out = h.run(&["telemetry", "collect", "--once"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("removed stale telemetry lock"));
    let ev = events(&h);
    assert!(close(total(&ev), 0.1565026), "stage 2 total {}", total(&ev));
    no_duplicate_keys(&ev);
    pass("3 stage 2 totals equal EXPECTED.md, no duplicate key");
    pass("4 a collector killed mid-tick loses and repeats nothing");

    // 5. The Claude probe, against today's real state.
    h.set("HORCH_FAKE_SCENARIO", "usage_exhausted");
    h.set("HORCH_NOW", "2026-09-28T18:30:00Z");
    let q: Value =
        serde_json::from_slice(&h.run(&["quota", "--refresh", "--json"]).stdout).unwrap();
    assert_eq!(q["pools"]["claude"]["state"], "exhausted");
    let seven = q["pools"]["claude"]["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == "7d" && w["scope_model"].is_null())
        .unwrap()
        .clone();
    assert_eq!(seven["used"].as_f64(), Some(1.0));
    assert_eq!(seven["resets_at"], "2026-10-02T13:59:59Z");
    assert!(h.violations().is_empty(), "{:?}", h.violations());
    h.unset("HORCH_FAKE_SCENARIO");
    pass("5 quota --refresh: claude exhausted, 7d 1.00, no fake violation");

    // 6-8. The gate, on quota fixtures.
    h.set("HORCH_NOW", "2026-09-28T18:00:00Z");
    h.set("HORCH_PROJECT_DIR", "/work/alpha");
    h.set("HORCH_WORKSPACE_ID", "w1");
    let quota = |name: &str| {
        fixtures()
            .join("quota")
            .join(format!("{name}.json"))
            .to_string_lossy()
            .into_owned()
    };
    h.set("HORCH_QUOTA_FILE", quota("claude-exhausted-codex-ok"));
    let out = h.run(&["route", "opus"]);
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("SUBSTITUTED: opus runs on codex-sol"));
    pass("6 route opus: SUBSTITUTED on codex-sol");
    h.set("HORCH_QUOTA_FILE", quota("all-exhausted"));
    let out = h.run(&["route", "opus"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("REFUSED: opus cannot start."));
    pass("7 route opus: REFUSED, exit 3");
    let ledger = h.state.join("-work-alpha.json");
    let before = std::fs::read(&ledger).unwrap();
    let herdr_calls = h.calls_of("herdr").len();
    let out = h.run(&["spawn", "opus", "x", "--from-pane", "w1:p1"]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(std::fs::read(&ledger).unwrap(), before);
    assert_eq!(
        h.calls_of("herdr").len(),
        herdr_calls,
        "no herdr call, so no pane split"
    );
    h.unset("HORCH_QUOTA_FILE");
    h.unset("HORCH_WORKSPACE_ID");
    pass("8 spawn opus: exit 3, ledger unchanged, no pane split");

    // 9. One frame of the screen, from the stage 2 snapshot.
    let out = h.run(&["telemetry", "render", "--size", "120x40", "--window", "7d"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    check_golden("telemetry-e2e.txt", &String::from_utf8_lossy(&out.stdout));
    println!("{}", String::from_utf8_lossy(&out.stdout));
    pass("9 render 120x40 equals golden/telemetry-e2e.txt");

    // 10. Nothing that must not persist did (TEL-11).
    for p in h
        .state_files()
        .into_iter()
        .filter(|p| p.starts_with(h.state.join("telemetry")))
    {
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        for s in [
            "SENTINEL-CONTENT",
            "sentinel@example.invalid",
            "acct_",
            "user.email",
        ] {
            assert!(!text.contains(s), "{s} in {}", p.display());
        }
    }
    pass("10 no content or identity in the state dir");

    // 11. horch usage agrees with horch cost, record by record (TEL-10).
    for project in ["/work/alpha", "/work/beta"] {
        h.set("HORCH_PROJECT_DIR", project);
        let usage: Value =
            serde_json::from_slice(&h.run(&["usage", "--json", "--project", project]).stdout)
                .unwrap();
        let cost: Value = serde_json::from_slice(&h.run(&["cost", "--json"]).stdout).unwrap();
        for row in cost["rows"].as_array().unwrap() {
            let rec = usage["records"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["record_id"] == row["record_id"])
                .unwrap();
            for class in [
                "input",
                "cache_write_5m",
                "cache_write_1h",
                "cache_read",
                "output",
            ] {
                assert_eq!(
                    rec["tokens"][class], row["tokens"][class],
                    "{} {class}",
                    row["record_id"]
                );
            }
            assert!(close(
                rec["cost_usd"].as_f64().unwrap(),
                row["cost"].as_f64().unwrap()
            ));
        }
    }
    pass("11 usage --json equals cost --json per record");
}
