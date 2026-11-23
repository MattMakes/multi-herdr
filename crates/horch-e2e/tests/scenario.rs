use std::collections::HashSet;
use std::path::Path;

use horch_core::execution::records::SessionLedger;
use horch_e2e::harness::{copy_tree, fixtures, wait_for_or, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

fn pass(step: &str) {
    println!("PASS {step}");
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

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

fn stop_fleetd_and_jobs(h: &Harness) {
    use horch_core::procid::{signal_same, start_time};
    h.stop_fleetd();
    let marks: Vec<String> = [Some(h.root.clone()), std::fs::canonicalize(&h.root).ok()]
        .into_iter()
        .flatten()
        .map(|root| format!("{}/", root.display()))
        .collect();
    let jobs = || -> Vec<(u32, u64)> {
        horch_e2e::process::commands_with_environment()
            .into_iter()
            .filter(|(_, l)| {
                l.contains(" job run ") && marks.iter().any(|m| l.contains(m.as_str()))
            })
            .filter_map(|(pid, _)| Some((pid, start_time(pid)?)))
            .collect()
    };
    let first = std::time::Instant::now();
    horch_e2e::harness::wait_for("the jobs of fleetd to end", || {
        let running = jobs();
        let signal = if first.elapsed() < std::time::Duration::from_secs(10) {
            15
        } else {
            9
        };
        for (pid, started) in &running {
            signal_same(*pid, *started, signal);
        }
        running.is_empty().then_some(())
    });
}

#[test]
fn the_hermetic_story() {
    story(false);
}

#[test]
fn the_hermetic_story_with_an_ollama_that_starts_in_7_s() {
    story(true);
}

fn refused_launch_changes_nothing() {
    let mut g = Harness::new("story-refused").with_git();
    g.set(
        "HORCH_LEDGER_DB",
        g.state.join("ledger.sqlite3").to_string_lossy(),
    );
    g.set("HORCH_NO_WAKE", "1").set("HORCH_NO_WATCHDOG", "1");
    g.set("HORCH_NOW", "2026-09-28T18:00:00Z");
    g.set(
        "HORCH_PROBE_TIMEOUT_MS",
        horch_e2e::harness::DEADLINE.as_millis().to_string(),
    );
    g.set(
        "HORCH_QUOTA_FILE",
        fixtures()
            .join("quota/all-exhausted.json")
            .to_string_lossy(),
    );
    g.set("HORCH_FAKE_SCENARIO", "exec");
    let root = g.fleet_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("fleet.toml"),
        "health_max_load_per_cpu = 1000000.0\npass_period_s = 1\n",
    )
    .unwrap();
    std::fs::write(
        g.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    for args in [
        &["add", "fleet.toml"][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
    ] {
        assert!(g.git_cmd(args).output().unwrap().status.success());
    }
    let workspace = g.fleet_workspace();
    assert_eq!(workspace, "w1");
    g.set("HORCH_WORKSPACE_ID", workspace);

    let ticket = g.project.join("opus-1.md");
    std::fs::write(&ticket, "# Opus\n**Check:** `true`\n").unwrap();
    let run = seed_run(&g, "refused", Some(1), "w1:p1");
    seed_task(&g, &run, &ticket, "opus");
    g.serve_fleetd();
    let shown = || {
        let o = g.run(&["ledger", "show", "T-1", "--json"]);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    wait_for_or(
        "the refusal of the launch",
        || {
            shown()
                .contains("its spawn failed: REFUSED: opus cannot start.")
                .then_some(())
        },
        shown,
    );
    let ledger = SessionLedger::for_project(&g.state, &g.project.to_string_lossy());
    assert!(ledger.read().unwrap().is_empty(), "no session record");
    let pane_calls: Vec<String> = g
        .calls_of("herdr")
        .iter()
        .map(|c| c["argv"].to_string())
        .filter(|a| a.contains("\"split\"") || a.contains("\"run\""))
        .collect();
    assert!(
        pane_calls.is_empty(),
        "no pane split or run: {pane_calls:?}"
    );
    assert!(!g
        .state
        .join("herdr-orchestration-w1")
        .join("opus-1.brief.json")
        .exists());
    stop_fleetd_and_jobs(&g);
}

fn claude_probe_reads_exhausted(h: &mut Harness) {
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
}

fn nothing_persists(h: &Harness) {
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
}

fn usage_agrees_with_cost(h: &mut Harness) {
    for project in ["/work/alpha", "/work/beta"] {
        h.set("HORCH_PROJECT_DIR", project);
        let usage: Value =
            serde_json::from_slice(&h.run(&["usage", "--json", "--project", project]).stdout)
                .unwrap();
        let one_report_of_every_call = ["cost", "--json", "--since", "2000-01-01"];
        let cost: Value = serde_json::from_slice(&h.run(&one_report_of_every_call).stdout).unwrap();
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
}

fn story(slow_ollama: bool) {
    let pass = |step: &str| {
        if !slow_ollama {
            pass(step);
        }
    };
    let mut h = Harness::new("story");
    if slow_ollama {
        h.set("HORCH_FAKE_OLLAMA_DELAY_MS", "7000");
    }
    let f = fixtures();
    let places = [
        (f.join("claude"), h.home.join(".claude/projects")),
        (f.join("codex/sessions"), h.home.join(".codex/sessions")),
        (f.join("pi/sessions"), h.home.join(".pi/agent/sessions")),
        (f.join("prime"), h.state.join("prime")),
    ];

    h.set("HORCH_NOW", "2026-09-28T18:00:00Z");
    h.set(
        "HORCH_PROBE_TIMEOUT_MS",
        horch_e2e::harness::DEADLINE.as_millis().to_string(),
    );
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

    claude_probe_reads_exhausted(&mut h);
    pass("5 quota --refresh: claude exhausted, 7d 1.00, no fake violation");

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
    refused_launch_changes_nothing();
    h.unset("HORCH_QUOTA_FILE");
    h.unset("HORCH_WORKSPACE_ID");
    pass("8 launch of opus: REFUSED, no record, no pane split");

    let out = h.run(&["telemetry", "render", "--size", "120x40", "--window", "7d"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    check_golden("telemetry-e2e.txt", &String::from_utf8_lossy(&out.stdout));
    if !slow_ollama {
        println!("{}", String::from_utf8_lossy(&out.stdout));
    }
    assert!(
        !h.calls_of("ollama").is_empty(),
        "the local check ran ollama"
    );
    pass("9 render 120x40 equals golden/telemetry-e2e.txt");

    nothing_persists(&h);
    pass("10 no content or identity in the state dir");

    usage_agrees_with_cost(&mut h);
    pass("11 usage --json equals cost --json per record");
}
