//! Behavioral coverage for fleet pitfalls #49–55.
use horch_core::execution::records::SessionLedger;
use horch_e2e::harness::{fixtures, Harness};
use serde_json::json;

fn out(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn g49_offline_stale_cache_survives_age_pipe_and_no_update() {
    let mut h = Harness::new("g49-stale");
    std::fs::write(h.state.join("self-check.json"), json!({"checked_at":"2020-01-01T00:00:00Z","latest_tag":"v99.0.0","html_url":"https://example.invalid/release"}).to_string()).unwrap();
    h.set("HORCH_RELEASE_API", "http://192.0.2.1:1");
    h.run(&["inbox"]); // Warm macOS first-exec scanning before measuring the offline path.
    let start = std::time::Instant::now();
    let o = h.run(&["inbox"]);
    assert_eq!(err(&o).matches("stale horch").count(), 1, "{}", err(&o));
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    h.set("HORCH_NO_SELF_CHECK", "1");
    assert!(!err(&h.run(&["inbox"])).contains("stale horch"));
}

#[test]
fn g50_machine_fixture_accepts_pinned_load() {
    let fixture = json!({"os":"linux","arch":"x86_64","cpus":2,"load_average_1m":3.5,"mem_total_bytes":null,"mem_available_bytes":null,"disk_free_bytes":null,"disk_total_bytes":null,"gpu":"unknown","max_open_files":null,"max_processes":null});
    let machine: horch_core::runtime::machine::MachineSnapshot =
        serde_json::from_value(fixture).expect("load fixture accepted");
    assert_eq!(
        serde_json::to_value(machine).unwrap()["load_average_1m"],
        3.5
    );
    let mut windows = serde_json::to_value(horch_core::runtime::machine::probe(
        std::path::Path::new("."),
        &Default::default(),
        Some(std::path::Path::new("/missing-fixture")),
    ))
    .unwrap();
    windows["os"] = json!("windows");
    let parsed: horch_core::runtime::machine::MachineSnapshot =
        serde_json::from_value(windows).unwrap();
    assert!(serde_json::to_value(parsed)
        .unwrap()
        .get("load_average_1m")
        .is_none());
}

#[test]
fn g51_sessions_filter_workspace_live_role_and_indent_summary() {
    let mut h = Harness::new("g51-sessions");
    let ledger = SessionLedger::for_project(&h.state, &h.project.to_string_lossy());
    for (id, role, workspace) in [("r1", "sonnet-1", "w1"), ("r2", "sonnet-2", "w2")] {
        ledger
            .add(horch_core::execution::records::NewRecord {
                record_id: id,
                agent: "claude",
                tier: "sonnet",
                model: "sonnet",
                role,
                session_id: Some(id),
                task: "first\nsecond\nthird",
                phase: None,
            })
            .unwrap();
        ledger
            .store()
            .update(|rows| {
                rows.iter_mut()
                    .find(|r| r.record_id == id)
                    .unwrap()
                    .workspace_id = Some(workspace.into());
                Ok(())
            })
            .unwrap();
    }
    h.set("HORCH_WORKSPACE_ID", "w1");
    let o = h.run(&["sessions", "--json"]);
    assert!(o.status.success(), "{}", err(&o));
    let rows: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1, "{}", out(&o));
    assert_eq!(rows[0]["record_id"], "r1");
    let o = h.run(&[
        "sessions",
        "--all-workspaces",
        "--role",
        "sonnet-2",
        "--live",
        "--json",
    ]);
    assert!(o.status.success(), "{}", err(&o));
    let rows: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["record_id"], "r2");
    let text = out(&h.run(&["sessions"]));
    assert!(
        text.contains("  task: first\n        second\n        third"),
        "{text}"
    );
}

#[test]
fn g51_sessions_points_to_ledger_status_only_for_the_callers_active_run() {
    let mut h = Harness::new("g51-pointer").with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_WORKSPACE_ID", "w1");
    assert!(!out(&h.run(&["sessions"])).contains("horch ledger status"));
    let o = h.run(&["ledger", "run", "new", "review"]);
    assert!(o.status.success(), "{}", err(&o));
    let text = out(&h.run(&["sessions"]));
    assert_eq!(
        text.lines().last(),
        Some("Active task-ledger run: horch ledger status"),
        "{text}"
    );
    h.set("HORCH_WORKSPACE_ID", "w2");
    assert!(!out(&h.run(&["sessions"])).contains("horch ledger status"));
}

#[test]
fn g54_doctor_warns_on_proxy_host() {
    let mut h = Harness::new("g54-proxy");
    h.set("HORCH_FAULT", "delivery-probe-ms:100");
    h.set("ANTHROPIC_BASE_URL", "https://proxy.example.invalid/v1");
    let o = h.run(&["doctor"]);
    assert!(o.status.success(), "{}", err(&o));
    let text = format!("{}{}", out(&o), err(&o));
    assert!(
        text.contains("proxy.example.invalid") && text.contains("cut short"),
        "{text}"
    );
    h.unset("ANTHROPIC_BASE_URL");
    let no_override = h.run(&["doctor"]);
    assert!(!format!("{}{}", out(&no_override), err(&no_override)).contains("cut short"));
    h.set("ANTHROPIC_BASE_URL", "https://api.anthropic.com");
    assert!(
        !format!("{}{}", out(&h.run(&["doctor"])), err(&h.run(&["doctor"]))).contains("cut short")
    );
}

#[test]
fn g55_unknown_pool_reason_is_visible() {
    let mut h = Harness::new("g55-reason");
    // A stale reading has no error and a table row without its reason: only
    // the `claude: <reason>` line says why the pool is unknown.
    let telemetry = h.state.join("telemetry");
    std::fs::create_dir_all(&telemetry).unwrap();
    std::fs::copy(
        fixtures().join("quota/claude-tight-codex-ok.json"),
        telemetry.join("quota.json"),
    )
    .unwrap();
    h.set("HORCH_NOW", "2026-09-28T18:30:00Z");
    let why = "reading from 2026-09-28T17:55:00Z is stale";
    let o = h.run(&["quota"]);
    assert!(o.status.success(), "{}", err(&o));
    let text = out(&o);
    assert!(
        text.lines().any(|l| l == format!("claude: {why}")),
        "{text}"
    );
    let j = h.run(&["quota", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&out(&j)).unwrap();
    assert_eq!(value["pools"]["claude"]["state"], "unknown");
    assert_eq!(value["pools"]["claude"]["reason"], why);
    // A probe error is the reason, in the row and on its own line.
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures()
            .join("quota/claude-unknown-codex-ok.json")
            .to_string_lossy(),
    );
    let text = out(&h.run(&["quota"]));
    let row = text.lines().find(|l| l.starts_with("claude")).unwrap();
    assert!(
        row.contains("UNKNOWN") && row.ends_with("probe timed out after 10s"),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|l| l == "claude: probe timed out after 10s"),
        "{text}"
    );
}

#[test]
fn g52_same_folder_rollouts_require_own_first_prompt_token() {
    use horch_core::harness::codex::{find_launch_rollouts, launch_token};
    let h = Harness::new("g52-token");
    let sessions = h.home.join(".codex/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let since = std::time::SystemTime::now() - std::time::Duration::from_secs(1);
    for (id, record) in [
        ("aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa", "r1"),
        ("bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb", "r2"),
    ] {
        std::fs::write(sessions.join(format!("rollout-stamp-{id}.jsonl")),format!("{}\n{}\n",json!({"type":"session_meta","payload":{"cwd":h.project}}),json!({"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":launch_token(record)}]}}))).unwrap();
    }
    for (record, id) in [
        ("r1", "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa"),
        ("r2", "bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb"),
    ] {
        let found = find_launch_rollouts(&sessions, &h.project.to_string_lossy(), since, record);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].session_id, id);
    }
    assert!(
        find_launch_rollouts(&sessions, &h.project.to_string_lossy(), since, "other").is_empty()
    );
}

#[test]
fn g55_real_logged_in_replies_have_known_expected_windows() {
    use horch_core::routing::quota::{parse_agy_usage, parse_claude_usage, parse_codex_limits};
    let root =
        horch_e2e::harness::repo_root().join("crates/horch-core/tests/fixtures/fleet18-quota");
    for (name, parser) in [
        (
            "claude-2.1.312",
            parse_claude_usage as fn(&serde_json::Value) -> _,
        ),
        ("codex-0.165.0", parse_codex_limits),
        ("agy-1.4.10", parse_agy_usage),
    ] {
        let value =
            serde_json::from_slice(&std::fs::read(root.join(format!("{name}.json"))).unwrap())
                .unwrap();
        let parsed = parser(&value).unwrap_or_else(|e| panic!("{name}: {e}"));
        let provenance: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join(format!("{name}-provenance.json"))).unwrap(),
        )
        .unwrap();
        assert!(provenance["source"]
            .as_str()
            .unwrap()
            .contains("real installed CLI"));
        assert!(provenance["version"].is_string());
        assert!(!parsed.windows.is_empty(), "{name}");
        if name.starts_with("claude") {
            assert_eq!(parsed.windows.len(), 3);
            assert!(parsed
                .windows
                .iter()
                .any(|w| w.name == "5h" && w.used == 0.0));
            assert!(parsed
                .windows
                .iter()
                .any(|w| w.name == "7d" && w.used == 1.0 && w.scope_model.is_none()));
            assert!(parsed
                .windows
                .iter()
                .any(|w| w.scope_model.as_deref() == Some("fable") && w.used == 0.0));
        } else if name.starts_with("codex") {
            assert_eq!(parsed.windows.len(), 1);
            assert_eq!(parsed.windows[0].name, "7d");
            assert_eq!(parsed.windows[0].used, 0.85);
            assert_eq!(parsed.ordinary_usage_allowed, Some(true));
        } else {
            assert_eq!(parsed.windows.len(), 9);
            assert!(parsed.windows.iter().all(|w| w.used == 0.0));
            assert!(parsed
                .windows
                .iter()
                .any(|w| w.scope_model.as_deref() == Some("argon")));
        }
    }
}

fn fleet(name: &str, scenario: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_FAULT", "delivery-probe-ms:100");
    h.set("HORCH_NO_WAKE", "1").set("HORCH_NO_WATCHDOG", "1");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_WORKSPACE_ID", "w1")
        .set("HORCH_FAKE_SCENARIO", scenario);
    herdr(&h, &["workspace", "create", "--label", name]);
    h
}
fn herdr(h: &Harness, args: &[&str]) -> serde_json::Value {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let o = cmd.args(args).output().unwrap();
    assert!(o.status.success(), "{}", err(&o));
    serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()["result"].clone()
}
fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "{} {}", out(&o), err(&o));
    out(&o)
}
fn wait<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn g52_concurrent_workers_get_their_own_session_and_transcript() {
    let h = fleet("g52-race", "exec,stay");
    ok(
        &h,
        &["ledger", "run", "new", "race", "--heartbeat-every", "1"],
    );
    let argv = [
        "spawn",
        "codex-terra",
        "--from-pane",
        "w1:p1",
        "--no-tile",
        "--force",
    ];
    // Launch within the same polling interval. Reverse worker traversal below
    // deliberately checks the worker that was created second first.
    let mut first = argv.to_vec();
    first.extend(["--role", "codex-terra-1"]);
    let mut second = argv.to_vec();
    second.extend(["--role", "codex-terra-2"]);
    let a = h
        .horch(&first)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let b = h
        .horch(&second)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    for child in [a, b] {
        let o = child.wait_with_output().unwrap();
        assert!(o.status.success(), "{}", err(&o));
    }
    let workers = wait("two session transcripts", || {
        let state: serde_json::Value =
            serde_json::from_str(&ok(&h, &["ledger", "status", "--json"])).ok()?;
        let ws: Vec<_> = state["workers"]
            .as_array()?
            .iter()
            .map(|w| w["worker"].clone())
            .filter(|w| w["kind"] == "worker")
            .collect();
        (ws.len() == 2
            && ws
                .iter()
                .all(|w| w["session_id"].is_string() && w["transcript_path"].is_string()))
        .then_some(ws)
    });
    assert_ne!(workers[0]["session_id"], workers[1]["session_id"]);
    assert_ne!(workers[0]["transcript_path"], workers[1]["transcript_path"]);
    let records = SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .read()
        .unwrap();
    for w in workers.iter().rev() {
        let record = records
            .iter()
            .find(|r| Some(r.record_id.as_str()) == w["legacy_record_id"].as_str())
            .unwrap();
        assert_eq!(record.session_id.as_deref(), w["session_id"].as_str());
        let transcript = std::fs::read_to_string(w["transcript_path"].as_str().unwrap()).unwrap();
        assert!(transcript.contains(&horch_core::harness::codex::launch_token(&record.record_id)));
    }
    // Exact PID/start identity protects cleanup from PID reuse.
    for w in &workers {
        let pid = w["agent_pid"].as_u64().unwrap() as u32;
        let started = w["agent_started"].as_u64().unwrap();
        assert!(horch_core::procid::signal_same(pid, started, 15));
    }
    wait("old launches exited", || {
        let current = ledger_records(&h);
        workers
            .iter()
            .all(|w| {
                current.iter().any(|r| {
                    Some(r.record_id.as_str()) == w["legacy_record_id"].as_str()
                        && !r.execution_status().is_live()
                })
            })
            .then_some(())
    });
    for (index, w) in workers.iter().enumerate() {
        let record = w["legacy_record_id"].as_str().unwrap();
        let session = w["session_id"].as_str().unwrap();
        let role = format!("codex-terra-resume-{index}");
        ok(
            &h,
            &[
                "spawn",
                "--resume",
                record,
                "continue",
                "--role",
                &role,
                "--from-pane",
                "w1:p1",
                "--no-tile",
                "--force",
            ],
        );
        wait("resume uses own session", || {
            h.calls_of("codex")
                .iter()
                .any(|call| call["resumed"] == session)
                .then_some(())
        });
    }
    let store = horch_core::taskledger::Store::open(&h.root.join("ledger.sqlite3")).unwrap();
    let mut query=store.conn().prepare("SELECT agent_pid,agent_started FROM workers WHERE status IN ('spawning','starting','claiming','working') AND agent_pid IS NOT NULL").unwrap();
    let pids = query
        .query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, u64>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for (pid, started) in pids {
        horch_core::procid::signal_same(pid, started, 15);
    }
}

fn ledger_records(h: &Harness) -> Vec<horch_core::execution::legacy::Record> {
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .read()
        .unwrap()
}

#[test]
fn g54_doctor_missing_mcp_and_hanging_docker_are_warnings() {
    let mut h = fleet("g54-services", "");
    let roster = h.root.join("roster");
    std::fs::create_dir_all(&roster).unwrap();
    std::fs::write(roster.join("service-test.md"),"---\nname: service-test\nbrief_description: Service check\nagent: claude\nmodel: sonnet\ndisallowed_tools: [Agent]\nrequires: [docker]\nmcp_servers:\n  absent-service:\n    command: definitely-not-installed\n  remote-http:\n    type: http\n    url: https://example.invalid/mcp\n    command: remote-command-must-not-be-checked\n---\nservice worker\n").unwrap();
    h.set("HORCH_TEAMMATES_DIR", roster.to_string_lossy());
    let marker = h.root.join("docker-started");
    h.write_bin(
        "docker",
        format!(
            "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\nprintf started > '{}'\nwhile :; do :; done\n",
            marker.display()
        )
        .as_bytes(),
    );
    let mut warm = std::process::Command::new(h.bin.join("docker"));
    h.seal(&mut warm);
    assert!(warm.arg("warm").output().unwrap().status.success());
    // Observe the daemon warning before later version checks complete.
    // Their own 15s budgets are independent of Docker's 2s timeout.
    use std::io::BufRead;
    let mut child = h
        .horch(&["doctor"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        for line in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            text.push_str(&line);
            text.push('\n');
            let _ = tx.send(line);
        }
        text
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
    let mut saw_timeout = false;
    while std::time::Instant::now() < deadline {
        if let Ok(line) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
            if line.contains("Docker daemon version timed out") {
                let since = std::fs::metadata(&marker)
                    .unwrap()
                    .modified()
                    .unwrap()
                    .elapsed()
                    .unwrap();
                assert!(
                    since < std::time::Duration::from_secs(4),
                    "Docker warning took {since:?}"
                );
                saw_timeout = true;
                break;
            }
        }
    }
    if !saw_timeout {
        let _ = child.kill();
        panic!("Docker timeout warning was not observed");
    }
    let o = child.wait_with_output().unwrap();
    let text = format!("{}{}", out(&o), reader.join().unwrap());
    assert!(o.status.success(), "{text}");
    assert!(text.contains("Docker daemon version timed out"), "{text}");
    assert!(
        text.contains("service-test: MCP server absent-service command definitely-not-installed"),
        "{text}"
    );
    assert_eq!(text.matches("MCP server absent-service").count(), 1);
    assert!(!text.contains("MCP server remote-http"), "{text}");
    let check = h.run(&["teammates", "--check"]);
    assert!(check.status.success(), "{}{}", out(&check), err(&check));
}

fn compact_world(name: &str, over: bool) -> (Harness, String) {
    let mut h = fleet(name, "");
    let sid = if over {
        "11111111-1111-4111-8111-111111111111"
    } else {
        "33333333-3333-4333-8333-333333333333"
    };
    let source =
        horch_e2e::harness::repo_root().join("crates/horch-core/tests/fixtures/context/home");
    h.copy_in(&source, &h.home);
    let pane = herdr(&h, &["pane", "split", "w1:p1", "--direction", "right"])["pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    herdr(
        &h,
        &[
            "pane",
            "test-state",
            &pane,
            "--status",
            "idle",
            "--agent",
            "claude",
        ],
    );
    herdr(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--status",
            "idle",
            "--agent",
            "claude",
        ],
    );
    let mailbox = horch_core::messaging::mailbox::Mailbox::under(&h.state, "w1");
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    std::fs::write(mailbox.dir().join("sonnet-1.id"), &pane).unwrap();
    std::fs::write(mailbox.dir().join("orchestrator.id"), "w1:p1").unwrap();
    let ledger = SessionLedger::for_project(&h.state, &h.project.to_string_lossy());
    ledger
        .add(horch_core::execution::records::NewRecord {
            record_id: "r-compact",
            agent: "claude",
            tier: "sonnet",
            model: "sonnet",
            role: "sonnet-1",
            session_id: Some(sid),
            task: "work",
            phase: None,
        })
        .unwrap();
    ledger
        .store()
        .update(|rows| {
            let r = rows
                .iter_mut()
                .find(|r| r.record_id == "r-compact")
                .unwrap();
            r.workspace_id = Some("w1".into());
            r.pane_id = Some(pane.clone());
            Ok(())
        })
        .unwrap();
    h.set("HORCH_RECORD_ID", "r-compact")
        .set("HORCH_ROLE", "sonnet-1");
    (h, pane)
}

#[test]
fn g53_progress_warns_once_and_missing_handoff_is_refused() {
    let (h, _) = compact_world("g53-missing", true);
    let a = h.run(&["note", "starting"]);
    assert!(a.status.success(), "{}", err(&a));
    assert!(out(&a).contains("Context warning"), "{}", out(&a));
    let b = h.run(&["note", "handoff: ai_docs/handoffs/sonnet-1-whats-next.md"]);
    assert!(b.status.success());
    assert!(
        err(&b).contains("handoff") && err(&b).contains("missing"),
        "{}{}",
        out(&b),
        err(&b)
    );
    assert!(!out(&b).contains("Context warning"));
    assert!(!h
        .calls_of("herdr")
        .iter()
        .any(|c| c["argv"].to_string().contains("/compact")));
}

#[test]
fn g53_below_threshold_handoff_does_not_schedule() {
    let (h, _) = compact_world("g53-small", false);
    let ledger = SessionLedger::for_project(&h.state, &h.project.to_string_lossy());
    ledger
        .record_event("r-compact", "context-warned", "earlier warning")
        .unwrap();
    let o = h.run(&["note", "handoff: ai_docs/handoffs/sonnet-1-whats-next.md"]);
    assert!(o.status.success());
    assert!(!out(&o).contains("scheduled"));
    assert!(!err(&o).contains("missing"));
}

#[test]
fn g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block() {
    let h = fleet("g50-runs", "");
    herdr(&h, &["workspace", "create", "--label", "other"]);
    let mut new = h.horch(&["ledger", "run", "new", "foreign"]);
    new.env("HORCH_WORKSPACE_ID", "w2");
    let o = new.output().unwrap();
    assert!(o.status.success(), "{}", err(&o));
    let o = h.run(&[
        "spawn",
        "sonnet",
        "task",
        "--from-pane",
        "w1:p1",
        "--no-tile",
        "--force",
    ]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(
        err(&o).contains("NOTE: live orchestrator in workspace w2 (run foreign)"),
        "{}",
        err(&o)
    );
    let o = h.run(&["fleet", "opus", "--no-update"]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(
        out(&o).contains(&format!("horch {}", env!("CARGO_PKG_VERSION"))),
        "{}",
        out(&o)
    );
    assert!(
        err(&o).contains("WARNING: live orchestrator in workspace w2 (run foreign)"),
        "{}",
        err(&o)
    );
    herdr(&h, &["workspace", "close", "w2"]);
    let o = h.run(&[
        "spawn",
        "sonnet",
        "task",
        "--from-pane",
        "w1:p1",
        "--no-tile",
        "--force",
    ]);
    assert!(!err(&o).contains("w2 (run foreign)"), "{}", err(&o));
}

#[test]
fn g50_legacy_orchestrator_and_host_load_are_advisory() {
    let mut h = fleet("g50-legacy", "");
    herdr(&h, &["workspace", "create", "--label", "legacy"]);
    let ledger = SessionLedger::for_project(&h.state, &h.project.to_string_lossy());
    ledger
        .insert(horch_core::execution::legacy::Record {
            record_id: "orchestrator-old".into(),
            kind: "orchestrator".into(),
            role: "orchestrator".into(),
            workspace_id: Some("w2".into()),
            ..Default::default()
        })
        .unwrap();
    let machine = h.root.join("machine.json");
    std::fs::write(&machine,json!({"os":"linux","arch":"x86_64","cpus":2,"load_average_1m":3.5,"mem_total_bytes":null,"mem_available_bytes":null,"disk_free_bytes":null,"disk_total_bytes":null,"gpu":"unknown","max_open_files":null,"max_processes":null}).to_string()).unwrap();
    h.set("HORCH_MACHINE_FILE", machine.to_string_lossy());
    let argv = [
        "spawn",
        "sonnet",
        "task",
        "--from-pane",
        "w1:p1",
        "--no-tile",
        "--force",
    ];
    let o = h.run(&argv);
    assert!(o.status.success(), "{}", err(&o));
    assert!(
        err(&o).contains("NOTE: live orchestrator in workspace w2"),
        "{}",
        err(&o)
    );
    assert!(
        err(&o).contains("load 3.50 is at or above 2 CPUs"),
        "{}",
        err(&o)
    );
    let mut below: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&machine).unwrap()).unwrap();
    below["load_average_1m"] = json!(1.0);
    std::fs::write(&machine, below.to_string()).unwrap();
    h.set("HORCH_WORKSPACE_ID", "w2");
    let o = h.run(&[
        "spawn",
        "sonnet",
        "task",
        "--from-pane",
        "w2:p1",
        "--no-tile",
        "--force",
    ]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(!err(&o).contains("live orchestrator"), "{}", err(&o));
    assert!(!err(&o).contains("1-minute load"), "{}", err(&o));
}

#[test]
fn g53_ledger_handoff_compacts_resumes_and_records_progress_once() {
    let (mut h, pane) = compact_world("g53-auto", true);
    ok(&h, &["ledger", "run", "new", "compact"]);
    let worker = ok(&h, &["ledger", "worker", "new", "--persona", "sonnet"])
        .trim()
        .to_string();
    h.set("HORCH_WORKER_ID", &worker);
    let db = horch_core::taskledger::Store::open(&h.root.join("ledger.sqlite3")).unwrap();
    db.conn()
        .execute(
            "UPDATE workers SET legacy_record_id='r-compact', status='working' WHERE worker_id=?1",
            [&worker],
        )
        .unwrap();
    let warning = ok(&h, &["ledger", "progress", "starting work"]);
    assert!(warning.contains("Context warning"), "{warning}");
    assert!(!ok(&h, &["ledger", "progress", "still working"]).contains("Context warning"));
    let transcript = horch_core::usage::Locations::from_context(
        &horch_core::runtime::RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new(&h.project).with("HOME", &h.home.to_string_lossy()),
        )
        .unwrap(),
    );
    let transcript = horch_core::telemetry::context::read_current(
        &transcript,
        &h.state.join("context"),
        "claude",
        Some("11111111-1111-4111-8111-111111111111"),
    )
    .unwrap()
    .transcript;
    h.set(
        "HORCH_FAKE_COMPACT_TRANSCRIPT",
        transcript.to_string_lossy(),
    );
    let handoff = h.project.join("ai_docs/handoffs/sonnet-1-whats-next.md");
    std::fs::create_dir_all(handoff.parent().unwrap()).unwrap();
    std::fs::write(&handoff, "# Handoff\ncontinue\n").unwrap();
    herdr(
        &h,
        &[
            "pane",
            "test-state",
            &pane,
            "--status",
            "working",
            "--agent",
            "claude",
        ],
    );
    let scheduled = ok(
        &h,
        &[
            "ledger",
            "progress",
            "handoff: ai_docs/handoffs/sonnet-1-whats-next.md",
        ],
    );
    assert!(scheduled.contains("scheduled"), "{scheduled}");
    let again = ok(&h, &["compact", "sonnet-1"]);
    assert!(again.contains("already running"), "{again}");
    herdr(
        &h,
        &[
            "pane",
            "test-state",
            &pane,
            "--status",
            "idle",
            "--agent",
            "claude",
        ],
    );
    let ledger = SessionLedger::for_project(&h.state, &h.project.to_string_lossy());
    wait("completed compaction", || {
        ledger
            .get("r-compact")
            .ok()?
            .history
            .iter()
            .any(|e| e.event == "compacted")
            .then_some(())
    });
    let calls = h.calls_of("herdr");
    let lines: Vec<_> = calls
        .iter()
        .filter_map(|c| c["argv"].as_array())
        .filter(|a| {
            a.first().and_then(|x| x.as_str()) == Some("agent")
                && a.get(1).and_then(|x| x.as_str()) == Some("prompt")
        })
        .filter_map(|a| a.get(3).and_then(|x| x.as_str()))
        .collect();
    assert_eq!(
        lines.iter().filter(|l| l.starts_with("/compact")).count(),
        1,
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("Read ai_docs/handoffs/sonnet-1-whats-next.md now. Then continue")),
        "{lines:?}"
    );
    // #57: only the orchestrator resume carries the command card.
    assert!(
        !lines
            .iter()
            .any(|l| l.contains("Command card:") || l.contains("decisions.md")),
        "{lines:?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("compaction started for sonnet-1"))
            .count(),
        1,
        "{lines:?}"
    );
    let (source, text): (String, String) = wait("task-ledger compaction progress", || {
        db.conn().query_row("SELECT w.last_progress_source,e.text FROM workers w JOIN events e ON e.worker_id=w.worker_id WHERE w.worker_id=?1 AND e.text LIKE 'compaction completed:%'",[&worker],|r|Ok((r.get(0)?,r.get(1)?))).ok()
    });
    assert_eq!(source, "transcript");
    assert!(text.contains("1201"));
}

#[test]
fn g49_spawn_doctor_and_inbox_warn_once_and_name_installed_binary() {
    let h = fleet("g49-path", "");
    h.write_bin("horch", b"#!/bin/sh\nexit 0\n");
    std::fs::write(h.state.join("self-check.json"),json!({"checked_at":"2020-01-01T00:00:00Z","latest_tag":"v99.0.0","html_url":"https://example.invalid"}).to_string()).unwrap();
    for argv in [
        &[
            "spawn",
            "sonnet",
            "task",
            "--from-pane",
            "w1:p1",
            "--no-tile",
            "--force",
        ][..],
        &["doctor"],
        &["inbox"],
    ] {
        let o = h.run(argv);
        assert!(o.status.success(), "{}{}", out(&o), err(&o));
        assert_eq!(err(&o).matches("stale horch").count(), 1, "{}", err(&o));
        assert!(
            err(&o).contains(&h.bin.join("horch").to_string_lossy().to_string()),
            "{}",
            err(&o)
        );
        assert!(
            err(&o).contains(&Harness::horch_bin().to_string_lossy().to_string()),
            "{}",
            err(&o)
        );
    }
}

#[test]
fn g51_pane_gone_context_filters_and_completed_summaries() {
    let mut h = fleet("g51-context", "");
    let ledger = SessionLedger::for_project(&h.state, &h.project.to_string_lossy());
    for (id, role, ws) in [
        ("a", "sonnet-1", "w1"),
        ("b", "sonnet-2", "w2"),
        ("same-role-other-fleet", "sonnet-1", "w2"),
        ("done", "sonnet-3", "w1"),
    ] {
        ledger
            .insert(horch_core::execution::legacy::Record {
                record_id: id.into(),
                role: role.into(),
                agent: "claude".into(),
                tier: "sonnet".into(),
                model: "sonnet".into(),
                workspace_id: Some(ws.into()),
                pane_id: Some("absent-pane".into()),
                ..Default::default()
            })
            .unwrap();
    }
    ledger.done("done", "one\ntwo\nthree").unwrap();
    let o = h.run(&["sessions", "--json"]);
    let rows: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(rows[0]["pane_gone"], true);
    let o = h.run(&["sessions", "--live", "--json"]);
    let rows: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert!(out(&h.run(&["sessions"])).contains("(pane gone)"));
    let text = out(&h.run(&["sessions"]));
    assert!(
        text.lines()
            .any(|l| l.starts_with("  ") && l.ends_with("two")),
        "{text}"
    );
    for (argv, count) in [
        (&["context", "--json"][..], 1),
        (&["context", "--all-workspaces", "--json"][..], 3),
        (
            &[
                "context",
                "--all-workspaces",
                "--role",
                "sonnet-2",
                "--live",
                "--json",
            ][..],
            1,
        ),
    ] {
        let o = h.run(argv);
        assert!(o.status.success(), "{}", err(&o));
        let rows: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), count, "{}", out(&o));
    }
    h.unset("HORCH_WORKSPACE_ID");
    let o = h.run(&["sessions", "--json"]);
    let rows: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 4);
}

#[test]
fn g55_unparseable_probe_reports_parser_error_in_quota() {
    let mut h = Harness::new("g55-unparse");
    h.set("HORCH_FAKE_SCENARIO", "error");
    h.set("HORCH_PROBE_TIMEOUT_MS", "2000");
    for name in ["claude", "codex", "agy"] {
        let mut cmd = std::process::Command::new(h.bin.join(name));
        h.seal(&mut cmd);
        assert!(cmd.arg("--version").output().unwrap().status.success());
    }
    let o = h.run(&["quota", "--refresh", "--json"]);
    assert!(o.status.success(), "{}", err(&o));
    let value: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(value["pools"]["claude"]["state"], "unknown");
    let reason = value["pools"]["claude"]["reason"].as_str().unwrap();
    assert!(
        reason.contains("rate_limits") || reason.contains("windows"),
        "{reason}"
    );
    assert!(out(&h.run(&["quota"])).contains(reason));
}

#[cfg(target_os = "linux")]
#[test]
fn g54_sandbox_warning_matches_launch_on_unsupported_host() {
    let mut h = fleet("g54-sandbox", "");
    let roster = h.root.join("roster");
    std::fs::create_dir_all(&roster).unwrap();
    std::fs::write(roster.join("sandbox-test.md"),"---\nname: sandbox-test\nbrief_description: Sandboxed worker\nagent: claude\nmodel: sonnet\nsandbox:\n  enabled: true\n---\nworker\n").unwrap();
    h.set("HORCH_TEAMMATES_DIR", roster.to_string_lossy());
    let o = h.run(&["doctor"]);
    assert!(o.status.success());
    assert!(
        err(&o).contains(
            "sandbox-test: refusing to launch without a sandbox: bwrap and socat not on PATH"
        ),
        "{}",
        err(&o)
    );
}

#[test]
fn g52_first_prompt_event_wins_over_context_and_later_turns_never_match() {
    use horch_core::harness::codex::{find_launch_rollouts, launch_token};
    let h = Harness::new("g52-context");
    let sessions = h.home.join(".codex/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let rows = [
        json!({"type":"session_meta","payload":{"cwd":h.project}}),
        json!({"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"AGENTS/environment context"}]}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":launch_token("r1")}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":launch_token("r2")}}),
    ];
    let body = rows
        .iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        sessions.join("rollout-stamp-aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa.jsonl"),
        body,
    )
    .unwrap();
    let since = std::time::SystemTime::now() - std::time::Duration::from_secs(1);
    assert_eq!(
        find_launch_rollouts(&sessions, &h.project.to_string_lossy(), since, "r1").len(),
        1
    );
    assert!(find_launch_rollouts(&sessions, &h.project.to_string_lossy(), since, "r2").is_empty());
    assert!(find_launch_rollouts(&sessions, &h.project.to_string_lossy(), since, "r").is_empty());
}

#[test]
fn g52_done_discovery_uses_the_same_launch_token() {
    let mut h = fleet("g52-done", "");
    let role = "codex-terra-1";
    let pane = ok(
        &h,
        &[
            "spawn",
            "codex-terra",
            "work",
            "--role",
            role,
            "--from-pane",
            "w1:p1",
            "--no-tile",
            "--force",
        ],
    )
    .trim()
    .to_string();
    let ledger = SessionLedger::for_project(&h.state, &h.project.to_string_lossy());
    let record = ledger
        .read()
        .unwrap()
        .into_iter()
        .find(|r| r.role == role)
        .unwrap();
    let mailbox = horch_core::messaging::mailbox::Mailbox::under(&h.state, "w1");
    std::fs::write(mailbox.dir().join(format!(".{role}.launch-marker")), "").unwrap();
    let sessions = h.home.join(".codex/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    for (sid, rid) in [
        (
            "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa",
            record.record_id.as_str(),
        ),
        ("bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb", "another-launch"),
    ] {
        std::fs::write(sessions.join(format!("rollout-stamp-{sid}.jsonl")),format!("{}\n{}\n",json!({"type":"session_meta","payload":{"cwd":h.project}}),json!({"type":"event_msg","payload":{"type":"user_message","message":horch_core::harness::codex::launch_token(rid)}}))).unwrap();
    }
    h.set("HORCH_RECORD_ID", &record.record_id)
        .set("HORCH_ROLE", role)
        .set("HERDR_PANE_ID", pane);
    ok(&h, &["done", "finished"]);
    assert_eq!(
        ledger.get(&record.record_id).unwrap().session_id.as_deref(),
        Some("aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa")
    );
}

#[test]
fn g55_real_captures_scrub_personal_identifiers_preserve_functional_limits() {
    fn has_reset_credit_id(text: &str) -> bool {
        text.split("RateLimitResetCredit_")
            .skip(1)
            .any(|suffix| suffix.bytes().take_while(u8::is_ascii_hexdigit).count() >= 16)
    }
    // A representative synthetic identifier proves the detector is active.
    assert!(has_reset_credit_id(
        "id: RateLimitResetCredit_0123456789abcdef0123456789abcdef"
    ));
    let root =
        horch_e2e::harness::repo_root().join("crates/horch-core/tests/fixtures/fleet18-quota");
    for entry in std::fs::read_dir(&root).unwrap().flatten() {
        let body = std::fs::read_to_string(entry.path()).unwrap();
        assert!(
            !has_reset_credit_id(&body),
            "unscrubbed identifier in {}",
            entry.path().display()
        );
    }
    let reply: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("codex-0.165.0.json")).unwrap()).unwrap();
    assert_eq!(reply["result"]["rateLimits"]["limitId"], "codex");
    assert!(reply["result"]["rateLimitsByLimitId"]["codex"].is_object());
    assert_eq!(
        reply["result"]["rateLimitResetCredits"]["credits"][0]["id"],
        "scrubbed-reset-credit-1"
    );
    assert_eq!(
        reply["result"]["rateLimitResetCredits"]["credits"][1]["id"],
        "scrubbed-reset-credit-2"
    );
}

#[test]
fn g53_first_handoff_without_prior_warning_does_not_schedule() {
    let (h, _) = compact_world("g53-no-ask", true);
    let o = h.run(&["note", "handoff: ai_docs/handoffs/sonnet-1-whats-next.md"]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("Context warning"));
    assert!(!out(&o).contains("scheduled"));
    assert!(!err(&o).contains("missing"));
    assert!(!h
        .calls_of("herdr")
        .iter()
        .any(|c| c["argv"].to_string().contains("/compact")));
}
