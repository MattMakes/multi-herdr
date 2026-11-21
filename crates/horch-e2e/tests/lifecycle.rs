#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

use horch_core::taskledger::runs::Run;
use horch_e2e::harness::{fixtures, repo_root, wait_for_or, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

const FORBIDDEN: &str = horch_platform::start::FORBIDDEN_ENV[0];

const AGY_KEYS: [&str; 3] = ["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_GEMINI_BASE_URL"];

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn world(name: &str, scenario: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WAKE", "1").set("HORCH_NO_WATCHDOG", "1");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_FAKE_SCENARIO", scenario);
    let root = h.fleet_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("fleet.toml"),
        "health_max_load_per_cpu = 1000000.0\npass_period_s = 1\n",
    )
    .unwrap();
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    for args in [
        &["add", "fleet.toml"][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
    ] {
        let o = h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
    }
    let workspace = h.fleet_workspace();
    assert_eq!(workspace, "w1");
    h.set("HORCH_WORKSPACE_ID", workspace);
    h
}

fn start(h: &Harness, persona: &str) -> Run {
    let ticket = h.project.join("t-1.md");
    std::fs::write(&ticket, "# Lifecycle\n**Check:** `true`\n").unwrap();
    let run = seed_run(h, "life", Some(1), "w1:p1");
    seed_task(h, &run, &ticket, persona);
    h.serve_fleetd();
    run
}

fn with_agy(h: &mut Harness) {
    let name = format!("agy{}", std::env::consts::EXE_SUFFIX);
    let bin = h.bin.join(name).to_string_lossy().into_owned();
    h.set("HORCH_ANTIGRAVITY_BIN", bin);
    let all_ok = std::fs::read_to_string(fixtures().join("quota/all-ok.json")).unwrap();
    let mut quota: Value = serde_json::from_str(&all_ok).unwrap();
    quota["pools"]["google"] = serde_json::json!({
        "state": "ok",
        "observed_at": "2026-09-28T17:55:00Z",
        "source": "usage",
        "windows": [{
            "name": "7d",
            "minutes": 10080,
            "scope_model": "gemini",
            "used": 0.1,
            "resets_at": "2026-10-02T13:59:59Z"
        }],
        "error": null
    });
    let path = h.root.join("quota-google-ok.json");
    std::fs::write(&path, quota.to_string()).unwrap();
    h.set("HORCH_QUOTA_FILE", path.to_string_lossy());
}

fn context(h: &Harness) -> String {
    let calls = h
        .calls()
        .iter()
        .filter(|c| c["fake"] != "herdr")
        .map(|c| format!("{} {}", c["fake"], c["argv"]))
        .collect::<Vec<_>>()
        .join("\n");
    let attempts = try_rows(
        h,
        "SELECT id || ' ' || state || ' ' || COALESCE(outcome, '') FROM attempts ORDER BY id",
    );
    let moves = try_rows(
        h,
        "SELECT at || ' ' || entity || ' ' || entity_id || ': ' || COALESCE(from_state, '')
             || ' -> ' || COALESCE(to_state, '') || ' (' || COALESCE(cause, '') || ')'
         FROM events WHERE to_state IS NOT NULL ORDER BY event_id",
    );
    format!(
        "attempts: {attempts:?}\nmoves:\n{}\ncalls:\n{calls}",
        moves.join("\n")
    )
}

fn try_rows(h: &Harness, sql: &str) -> Vec<String> {
    let read = || -> rusqlite::Result<Vec<String>> {
        let conn = rusqlite::Connection::open(h.ledger_db())?;
        let mut stmt = conn.prepare(sql)?;
        let found = stmt.query_map([], |r| r.get::<_, String>(0))?;
        found.collect()
    };
    read().unwrap_or_else(|e| vec![format!("the ledger read failed: {e}")])
}

fn launch_of(h: &Harness, fake: &str) -> Value {
    let tasks = h.fleet_root().join("tasks").to_string_lossy().into_owned();
    wait_for_or(
        &format!("the launch of {fake}"),
        || {
            h.calls_of(fake)
                .into_iter()
                .find(|c| c["argv"].to_string().contains(&tasks))
        },
        || context(h),
    )
}

fn pane_of(h: &Harness) -> String {
    wait_for_or(
        "the pane run of the attempt",
        || {
            h.calls_of("herdr").into_iter().find_map(|c| {
                let argv = c["argv"].as_array()?;
                (argv.first()? == "pane" && argv.get(1)? == "run")
                    .then(|| argv[2].as_str().map(str::to_owned))?
            })
        },
        || context(h),
    )
}

fn close_pane(h: &Harness, pane: &str) {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let out = cmd.args(["pane", "close", pane]).output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
}

fn attempt_state(h: &Harness) -> Option<String> {
    let conn = rusqlite::Connection::open(h.ledger_db()).ok()?;
    conn.query_row("SELECT state FROM attempts", [], |r| r.get(0))
        .ok()
}

fn rows(h: &Harness, sql: &str) -> Vec<String> {
    let conn = rusqlite::Connection::open(h.ledger_db()).unwrap();
    let mut stmt = conn.prepare(sql).unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

fn session_of(h: &Harness) -> Value {
    wait_for_or(
        "the session of the attempt",
        || {
            let recorded = rows(h, "SELECT text FROM events WHERE kind = 'session'");
            let file = files_named(&h.fleet_root().join("tasks"), "session.json")
                .into_iter()
                .next()?;
            let session: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
            let id = session["session_id"].as_str().filter(|s| !s.is_empty())?;
            recorded.iter().any(|t| t.contains(id)).then_some(session)
        },
        || {
            format!(
                "{}\nevents: {:?}",
                context(h),
                rows(h, "SELECT kind || ' ' || text FROM events")
            )
        },
    )
}

fn files_named(dir: &std::path::Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if p.file_name().is_some_and(|n| n == name) {
                found.push(p);
            }
        }
    }
    found
}

fn lifecycle(harness: &str, persona: &str, fake: &str, records_session: bool) -> (Value, Value) {
    let mut h = world(&format!("arc26-{harness}"), "exec,stay");
    if fake == "antigravity" {
        with_agy(&mut h);
    }
    start(&h, persona);
    let launch = launch_of(&h, fake);
    assert_eq!(launch["violations"], serde_json::json!([]), "{launch}");

    let session = if records_session {
        let session = session_of(&h);
        assert_eq!(session["harness"], harness, "{session}");
        session
    } else {
        Value::Null
    };

    wait_for_or(
        "the attempt to be live",
        || (attempt_state(&h).as_deref() == Some("live")).then_some(()),
        || context(&h),
    );
    close_pane(&h, &pane_of(&h));
    wait_for_or(
        "the attempt to end",
        || (attempt_state(&h).as_deref() == Some("ended")).then_some(()),
        || context(&h),
    );
    (launch, session)
}

#[test]
fn arc_26_e2e_lifecycle_matrix_claude() {
    lifecycle("claude", "sonnet", "claude", true);
}

#[test]
fn arc_26_e2e_lifecycle_matrix_codex() {
    lifecycle("codex", "codex-sol", "codex", false);
}

#[test]
fn arc_26_e2e_lifecycle_matrix_opencode() {
    lifecycle("opencode", "opencode-pickle", "opencode", true);
}

#[test]
fn arc_26_e2e_lifecycle_matrix_pi() {
    lifecycle("pi", "pi", "pi", true);
}

#[test]
fn arc_26_e2e_lifecycle_matrix_prime() {
    lifecycle("prime", "prime", "prime", true);
}

#[test]
fn arc_26_e2e_lifecycle_matrix_antigravity() {
    lifecycle("antigravity", "antigravity", "antigravity", false);
}

#[test]
fn pane_command_removes_a_forbidden_key_before_horch_starts() {
    let mut h = world("pane-env", "default");
    h.set(FORBIDDEN, "dummy-forbidden-value");
    start(&h, "sonnet");
    let ran = wait_for_or(
        "a pane command",
        || {
            h.calls_of("herdr")
                .into_iter()
                .find_map(|c| c["ran"].as_str().map(str::to_owned))
        },
        || context(&h),
    );
    assert!(
        ran.starts_with(&format!("exec /usr/bin/env -u {FORBIDDEN} ")),
        "the pane command does not remove {FORBIDDEN}: {ran}"
    );

    let horch = format!("'{}'", Harness::horch_bin().display());
    assert!(ran.contains(&horch), "the pane command has no {horch}");
    let out_file = h.tmp.join("pane-probe.env");
    let probe = h.write_warmed(
        "pane-probe",
        b"#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\n/usr/bin/env > \"$PANE_PROBE_OUT\"\n",
    );
    let line = ran.replacen(&horch, &format!("'{}'", probe.display()), 1);
    let mut shell = std::process::Command::new("/bin/sh");
    h.seal(&mut shell);
    shell.arg("-c").arg(&line).env("PANE_PROBE_OUT", &out_file);
    let out = shell.output().unwrap();
    assert!(out.status.success(), "{line}\n{}", text(&out));
    let env = std::fs::read_to_string(&out_file).unwrap();
    let names: Vec<&str> = env
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, _)| k))
        .collect();
    assert!(names.contains(&"HORCH_LEDGER_DB"), "{names:?}");
    assert!(!names.contains(&FORBIDDEN), "the probe has {FORBIDDEN}");
}

#[test]
fn worker_warns_once_about_a_teammate_file_that_does_not_load() {
    let h = world("f7-worker", "exec");
    let dir = h.home.join(".config/horch/teammates");
    std::fs::create_dir_all(&dir).unwrap();
    let broken = std::fs::read_to_string(repo_root().join("teammates/opus.md"))
        .unwrap()
        .replace("name: opus", "name: newcomer")
        .replacen("\n---\n", "\nrequires: [no-such-tool]\n---\n", 1);
    std::fs::write(dir.join("newcomer.md"), broken).unwrap();
    start(&h, "sonnet");
    launch_of(&h, "claude");
    let pane = pane_of(&h);
    let mut out = h.log.clone().into_os_string();
    out.push(format!(".pane-{}.out", pane.replace(':', "_")));
    let stderr = wait_for_or(
        "the pane output",
        || {
            std::fs::read_to_string(&out)
                .ok()
                .filter(|t| t.contains("warning: teammate 'newcomer'"))
        },
        || context(&h),
    );
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|l| l.starts_with("warning: teammate 'newcomer'"))
        .collect();
    assert_eq!(warnings.len(), 1, "{stderr}");
    assert!(
        warnings[0].contains("unknown variant `no-such-tool`"),
        "{stderr}"
    );
}

fn launch_with_keys(name: &str, persona: &str, fake: &str, extra: &[&str]) -> Value {
    let mut h = world(name, "exec");
    if fake == "antigravity" {
        with_agy(&mut h);
    }
    let keys_set: Vec<&str> = std::iter::once(FORBIDDEN)
        .chain(extra.iter().copied())
        .collect();
    for key in &keys_set {
        h.set(key, "dummy-must-not-leak");
    }
    start(&h, persona);
    let launch = launch_of(&h, fake);
    let keys: Vec<String> = serde_json::from_value(launch["env_keys"].clone()).unwrap();
    for key in &keys_set {
        assert!(
            !keys.iter().any(|k| k == key),
            "{key} reached {fake}: {keys:?}"
        );
    }
    assert_eq!(launch["violations"], serde_json::json!([]), "{launch}");
    launch
}

#[test]
fn antigravity_child_never_receives_a_forbidden_key() {
    let launch = launch_with_keys("agy-env", "antigravity", "antigravity", &AGY_KEYS);
    let argv: Vec<String> = serde_json::from_value(launch["argv"].clone()).unwrap();
    assert!(
        argv.windows(2)
            .any(|w| w == ["--model", "gemini-3.8-flash"]),
        "{argv:?}"
    );
    assert!(
        argv.windows(2).any(|w| w == ["--effort", "medium"]),
        "{argv:?}"
    );
}

#[test]
fn codex_child_never_receives_a_forbidden_key() {
    launch_with_keys("codex-env", "codex-sol", "codex", &[]);
}

#[test]
fn the_model_of_the_persona_reaches_the_claude_command_line() {
    let h = world("persona-model-fable", "exec");
    start(&h, "fable-creative");
    let launch = launch_of(&h, "claude");
    let argv: Vec<String> = serde_json::from_value(launch["argv"].clone()).unwrap();
    assert!(
        argv.windows(2).any(|w| w == ["--model", "fable"]),
        "{argv:?}"
    );
}

#[test]
fn tel_session_recorded_when_prime_ends_fast() {
    let (launch, session) = lifecycle("prime", "prime", "prime", true);
    assert_eq!(session["session_id"], launch["session_file"], "{session}");
}

#[test]
fn arc_16_e2e_fail_split() {
    let h = world("arc16-split", "exec,fail_split");
    start(&h, "sonnet");
    wait_for_or(
        "the worker to be launch_failed",
        || {
            rows(&h, "SELECT status FROM workers")
                .iter()
                .any(|s| s == "launch_failed")
                .then_some(())
        },
        || context(&h),
    );
    assert!(
        !h.calls_of("herdr").iter().any(|c| c["ran"].is_string()),
        "a pane ran a command after the failed split"
    );
    let tasks = h.fleet_root().join("tasks").to_string_lossy().into_owned();
    assert!(
        !h.calls_of("claude")
            .iter()
            .any(|c| c["argv"].to_string().contains(&tasks)),
        "an agent launched after the failed split"
    );
    assert_ne!(attempt_state(&h).as_deref(), Some("live"));
}
