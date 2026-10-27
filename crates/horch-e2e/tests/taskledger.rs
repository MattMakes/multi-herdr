//! The task ledger at the CLI seam (#28): the real `horch` binary, the fake
//! herdr and fake agents, a temporary ledger file per test.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use horch_e2e::harness::{fixtures, Harness};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A harness with its own ledger file and a git repository.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h
}

/// A harness in workspace `w1` (root pane `w1:p1`) whose panes run their
/// commands, like `horch fleet` leaves it.
fn fleet(name: &str, scenario: &str) -> Harness {
    let mut h = world(name);
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", scenario);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    h
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn ok_as(h: &Harness, worker: &str, args: &[&str]) -> String {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", worker);
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn json_of(h: &Harness, args: &[&str]) -> Value {
    let o = h.run(args);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

fn ticket(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.md"));
    std::fs::write(&p, body).unwrap();
    p
}

fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

fn worker_new(h: &Harness, persona: &str) -> String {
    ok(h, &["ledger", "worker", "new", "--persona", persona])
        .trim()
        .to_string()
}

#[test]
fn status_on_a_fresh_ledger_creates_it_and_says_no_runs() {
    let h = world("tl-fresh");
    let db = h.root.join("ledger.sqlite3");
    assert!(!db.exists());
    let o = h.run(&["ledger", "status"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(out(&o).trim(), "No runs.");
    assert!(db.exists(), "status creates the ledger file");
    let o = h.run(&["ledger", "status", "--json"]);
    assert!(o.status.success());
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["runs"], Value::Array(vec![]));
}

#[test]
fn the_ledger_path_is_in_horch_home_by_default() {
    let mut h = Harness::new("tl-path");
    h.unset("HORCH_LEDGER_DB");
    let p = ok(&h, &["ledger", "path"]);
    assert_eq!(
        Path::new(p.trim()),
        h.home.join(".horch").join("ledger.sqlite3")
    );
}

#[test]
fn legacy_plumbing_lives_under_ledger_legacy() {
    let h = world("tl-legacy");
    let p = ok(&h, &["ledger", "legacy", "path"]);
    assert!(p.trim().ends_with(".json"), "{p}");
}

#[test]
fn queue_claim_and_status_through_the_cli() {
    let h = world("tl-queue");
    ok(&h, &["ledger", "run", "new", "alpha"]);
    // The run branch exists, from the checked-out branch.
    let mut git = h.git_cmd(&["branch", "--list", "fleet/alpha"]);
    git.current_dir(&h.project);
    let branches = git.output().unwrap();
    assert!(
        out(&branches).contains("fleet/alpha"),
        "{}",
        text(&branches)
    );

    let a = ticket(&h, "01-a", "# 01: Parser\n**Check:** `true`\n");
    let b = ticket(&h, "02-b", "# 02: Printer\n");
    let a = a.to_string_lossy().into_owned();
    let b = b.to_string_lossy().into_owned();
    let added = ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &a,
            "--primary",
            "opus",
            "--own",
            "src/parse",
        ],
    );
    assert!(added.contains("T-1 queued for opus: Parser."), "{added}");
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &b,
            "--primary",
            "opus",
            "--blocked-by",
            "T-1",
        ],
    );

    let plan = json_of(&h, &["ledger", "plan-workers", "--json"]);
    assert_eq!(plan[0]["persona"], "opus");
    assert_eq!(plan[0]["spawn"], 1, "{plan}");

    let o = h.run(&["ledger", "status"]);
    assert_eq!(o.status.code(), Some(1), "work remains: {}", text(&o));
    assert!(out(&o).contains("blocked by T-1 (queued)"), "{}", out(&o));

    let w1 = worker_new(&h, "opus");
    let claim = ok_as(&h, &w1, &["ledger", "claim"]);
    assert!(claim.contains("Claimed T-1 (attempt 1): Parser"), "{claim}");
    assert!(claim.contains("Ticket: tickets/01-a.md"), "{claim}");
    assert!(claim.contains("Own: src/parse"), "{claim}");
    assert!(
        claim.contains("horch ledger worktree new parser"),
        "{claim}"
    );

    let w2 = worker_new(&h, "opus");
    let none = ok_as(&h, &w2, &["ledger", "claim"]);
    assert!(
        none.contains("NOTE: No task for opus. I exit. PID"),
        "{none}"
    );

    ok_as(&h, &w1, &["ledger", "progress", "parser compiles"]);
    let shown = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    let kinds: Vec<&str> = shown["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["added", "claimed", "progress"]);

    ok(&h, &["ledger", "cancel", "T-2", "not needed"]);
    let st = json_of(&h, &["ledger", "status", "--json"]);
    assert_eq!(st["counts"]["cancelled"], 1);
    assert_eq!(st["complete"], false);
}

#[test]
fn add_map_queues_a_ticket_map_in_one_step() {
    let h = world("tl-map");
    ok(&h, &["ledger", "run", "new", "beta"]);
    ticket(
        &h,
        "01-a",
        "# 01: Store\n\n**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes\n",
    );
    ticket(
        &h,
        "02-b",
        "# 02: Claim\n\n**Persona:** primary `opus` · **Parallel:** no\n",
    );
    std::fs::write(
        h.project.join("tickets/MAP.md"),
        "# Map: x\nSpec: [#28](https://example/28)\n\n| # | Issue | Title | Blocked by | Wave |\n|---|---|---|---|---|\n\
         | [01](01-a.md) | [#29](https://example/29) | Store | - | 1 |\n\
         | [02](02-b.md) | [#30](https://example/30) | Claim | #29 | 2 |\n",
    )
    .unwrap();
    let o = ok(&h, &["ledger", "add-map", "tickets/MAP.md"]);
    assert!(o.contains("2 tasks queued"), "{o}");
    let st = json_of(&h, &["ledger", "status", "--json"]);
    let tasks = st["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 2);
    let claim = &tasks[1]["task"];
    assert_eq!(claim["claimable_by"], "opus");
    assert_eq!(claim["parallel"], false);
    assert_eq!(claim["issue_url"], "https://example/30");
    assert_eq!(claim["spec_ref"], "https://example/28");
    assert!(tasks[1]["waits"]
        .as_str()
        .unwrap()
        .contains("blocked by T-1"));

    // A map with an unknown persona line fails as a whole.
    ticket(&h, "03-c", "# 03: No persona\n");
    std::fs::write(
        h.project.join("tickets/BAD.md"),
        "| # | Title | Blocked by |\n|---|---|---|\n| [03](03-c.md) | C | - |\n",
    )
    .unwrap();
    let o = h.run(&["ledger", "add-map", "tickets/BAD.md"]);
    assert!(!o.status.success());
    assert!(
        text(&o).contains("names no primary persona"),
        "{}",
        text(&o)
    );
}

#[test]
fn eight_parallel_claim_processes_take_exactly_three_tasks() {
    let h = world("tl-race");
    ok(&h, &["ledger", "run", "new", "race"]);
    for n in 0..3 {
        let p = ticket(&h, &format!("t{n}"), &format!("# T{n}\n"));
        ok(
            &h,
            &[
                "ledger",
                "add",
                "--file",
                &p.to_string_lossy(),
                "--primary",
                "opus",
            ],
        );
    }
    let ids: Vec<String> = (0..8).map(|_| worker_new(&h, "opus")).collect();
    let children: Vec<_> = ids
        .iter()
        .map(|id| {
            let mut cmd = h.horch(&["ledger", "claim", "--json"]);
            cmd.env("HORCH_WORKER_ID", id);
            cmd.stdout(std::process::Stdio::piped());
            cmd.spawn().unwrap()
        })
        .collect();
    let mut claimed = 0;
    for c in children {
        let o = c.wait_with_output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
        let v: Value = serde_json::from_slice(&o.stdout).unwrap();
        if v["outcome"] == "claimed" {
            claimed += 1;
        }
    }
    assert_eq!(claimed, 3);
    let st = json_of(&h, &["ledger", "status", "--json"]);
    assert_eq!(st["counts"]["running"], 3);
}

fn worker_json(h: &Harness, id: &str) -> Value {
    json_of(h, &["ledger", "show", id, "--json"])["worker"].clone()
}

#[test]
fn a_spawned_worker_records_its_pids_session_and_checkins() {
    let mut h = fleet("tl-spawn", "exec,stay");
    h.set("HORCH_HOSTNAME", "test-host");
    ok(
        &h,
        &["ledger", "run", "new", "gamma", "--heartbeat-every", "1"],
    );
    let p = ticket(&h, "01-a", "# 01: A\n");
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            "codex-terra",
        ],
    );

    let o = h.run(&[
        "spawn",
        "codex-terra",
        "--from-pane",
        "w1:p1",
        "--no-tile",
        "--force",
    ]);
    assert!(o.status.success(), "{}", text(&o));
    let pane = out(&o).lines().last().unwrap().trim().to_string();
    let st = json_of(&h, &["ledger", "status", "--json"]);
    let w = st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["worker"]["kind"] == "worker")
        .expect("spawn wrote a worker row")["worker"]
        .clone();
    let id = w["worker_id"].as_str().unwrap().to_string();
    assert_eq!(w["persona"], "codex-terra");
    assert_eq!(w["pane_id"], pane.as_str());
    assert_eq!(w["host"], "test-host");

    // The supervisor and the agent record their PIDs; the start times match.
    let w = wait_for("both PIDs", || {
        let w = worker_json(&h, &id);
        (w["agent_pid"].is_i64() && w["supervisor_pid"].is_i64()).then_some(w)
    });
    for (pid, started) in [
        ("supervisor_pid", "supervisor_started"),
        ("agent_pid", "agent_started"),
    ] {
        let pid = w[pid].as_i64().unwrap() as u32;
        let want = horch_core::procid::start_time(pid).expect("the process runs");
        assert_eq!(w[started].as_i64().unwrap() as u64, want, "{pid}");
    }
    // The agent was told to claim.
    let launch = h
        .calls_of("codex")
        .into_iter()
        .rev()
        .find(|c| c["argv"].to_string().contains("horch ledger claim"));
    assert!(
        launch.is_some(),
        "the agent's task names horch ledger claim"
    );

    // Checkins advance while the agent runs.
    let first = w["last_checkin_at"].as_str().unwrap().to_string();
    wait_for("2 more checkins", || {
        let a = worker_json(&h, &id)["last_checkin_at"]
            .as_str()?
            .to_string();
        (a > first).then_some(())?;
        std::thread::sleep(Duration::from_millis(1100));
        let b = worker_json(&h, &id)["last_checkin_at"]
            .as_str()?
            .to_string();
        (b > a).then_some(())
    });
    // The discovered session and its transcript reach the row.
    let w = wait_for("the session", || {
        let w = worker_json(&h, &id);
        w["session_id"].is_string().then_some(w)
    });
    let w = if w["transcript_path"].is_string() {
        w
    } else {
        wait_for("the transcript", || {
            let w = worker_json(&h, &id);
            w["transcript_path"].is_string().then_some(w)
        })
    };
    assert!(
        Path::new(w["transcript_path"].as_str().unwrap()).is_file(),
        "{w}"
    );

    // The agent claims, then is killed: the task is queued again.
    ok_as(&h, &id, &["ledger", "claim"]);
    let agent = w["agent_pid"].as_i64().unwrap() as u32;
    let started = w["agent_started"].as_i64().unwrap() as u64;
    assert!(horch_core::procid::signal_same(
        agent,
        started,
        libc_sigterm()
    ));
    let w = wait_for("the exit", || {
        let w = worker_json(&h, &id);
        (w["status"] == "incomplete").then_some(w)
    });
    assert!(
        w["end_reason"]
            .as_str()
            .unwrap()
            .contains("before horch ledger done"),
        "{w}"
    );
    let checkin = w["last_checkin_at"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(
        worker_json(&h, &id)["last_checkin_at"].as_str().unwrap(),
        checkin,
        "checkins stop with the agent"
    );
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["status"], "queued");
    assert!(
        t["task"]["priority"].as_i64().unwrap() < 0,
        "first in line: {t}"
    );
}

fn libc_sigterm() -> i32 {
    15
}
