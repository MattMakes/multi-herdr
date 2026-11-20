//! End-to-end: a worker's life through the real `horch fleetd` and its
//! fake agents. fleetd launches each attempt; `horch spawn` is retired
//! (FR-5, `docs/tickets/fleetd-p6/47-c6-t908-retire-worker-run-cli.md`).
//!
//! - ARC-26: for each of the 6 harnesses, fleetd launches the agent of a
//!   seeded task, the wrapper records the session of the agent
//!   (`session.json`), and the attempt ends when its pane closes.
//! - ARC-16: a failed pane split launches nothing.
//! - G3: no child of fleetd holds a forbidden key, and the pane command
//!   removes the key before horch starts.
//!
//! fake-herdr runs in the `exec` scenario, so the pane command of an
//! attempt really starts `horch worker`, and `pane close` kills it. The
//! fake agents run in `stay` when a test needs a live agent.
//!
//! Where a test of the retired `horch spawn` has no form in the fleetd
//! flow, it is gone: `arc_16_crash_after_insert_not_live` (the fault
//! `abort-after-execution-insert` belonged to the record that `horch spawn`
//! wrote; the faults of fleetd are in `p1_pass.rs` and `p4_no_loss.rs`) and
//! the resume half of each test (`horch spawn --resume`).
//!
//! The forbidden key has a dummy value that no test prints. A test names
//! the key and never shows its value.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

use horch_core::taskledger::runs::Run;
use horch_e2e::harness::{fixtures, repo_root, wait_for_or, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

/// The forbidden name (`horch_platform::start::FORBIDDEN_ENV`).
const FORBIDDEN: &str = "ANTHROPIC_API_KEY";

/// The keys that move agy off the operator's Google login.
const AGY_KEYS: [&str; 3] = ["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_GEMINI_BASE_URL"];

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A harness in workspace `w1` (root pane `w1:p1`), as `horch fleet` leaves
/// it, with a git project that declares its gates and a fleet root whose
/// fleetd passes once each second. The test starts fleetd after it queued
/// its task.
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
    // The project declares its gates, so no hold of a missing fast gate
    // stops the dispatch.
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

/// Queue 1 task for `persona` in a new run, then start the test fleetd.
fn start(h: &Harness, persona: &str) -> Run {
    let ticket = h.project.join("t-1.md");
    std::fs::write(&ticket, "# Lifecycle\n**Check:** `true`\n").unwrap();
    let run = seed_run(h, "life", Some(1), "w1:p1");
    seed_task(h, &run, &ticket, persona);
    h.serve_fleetd();
    run
}

/// `fake-antigravity` as `agy` (the harness installs it in `bin/`), named by
/// `HORCH_ANTIGRAVITY_BIN`, and an ok google pool reading as `agy /usage`
/// gives it. `all-ok.json` has no google pool, and an unknown pool runs the
/// teammate on its fallback.
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

/// What a failed wait shows: the fake calls without the many herdr calls.
fn context(h: &Harness) -> String {
    h.calls()
        .iter()
        .filter(|c| c["fake"] != "herdr")
        .map(|c| format!("{} {}", c["fake"], c["argv"]))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The launch of the attempt by `fake`: the call whose argv names the
/// attempt folder under the fleet root. A probe call of the fake does not.
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

/// The pane that fleetd opened for the attempt: the argument of its
/// `pane run`.
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

/// `herdr pane close`, which kills the worker and its agent.
fn close_pane(h: &Harness, pane: &str) {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let out = cmd.args(["pane", "close", pane]).output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
}

/// The state of the 1 attempt of the ledger.
fn attempt_state(h: &Harness) -> Option<String> {
    let conn = rusqlite::Connection::open(h.ledger_db()).ok()?;
    conn.query_row("SELECT state FROM attempts", [], |r| r.get(0))
        .ok()
}

/// The rows of the query `sql` in the ledger, 1 text column.
fn rows(h: &Harness, sql: &str) -> Vec<String> {
    let conn = rusqlite::Connection::open(h.ledger_db()).unwrap();
    let mut stmt = conn.prepare(sql).unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

/// The `session.json` that the wrapper of the attempt wrote, once fleetd
/// ingested it into the ledger as a session event.
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

/// Each file named `name` under `dir`, at any depth.
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

/// Launch → session recorded → attempt live → pane closes → attempt ended.
/// Gives the launch call and the session of the attempt, or null for a
/// harness whose fake records no session in the exec launch of fleetd
/// (`records_session` false: codex and antigravity, see the tests).
fn lifecycle(harness: &str, persona: &str, fake: &str, records_session: bool) -> (Value, Value) {
    let mut h = world(&format!("arc26-{harness}"), "exec,stay");
    if fake == "antigravity" {
        with_agy(&mut h);
    }
    start(&h, persona);
    let launch = launch_of(&h, fake);
    assert_eq!(launch["violations"], serde_json::json!([]), "{launch}");

    // The session id is known: minted by horch (claude, pi) or discovered
    // while the agent runs.
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
    // fake-codex writes a rollout only in its TUI launch, not in the exec
    // launch of fleetd, so no session is recorded.
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

/// fake-antigravity records its conversation in the session cache only in
/// its TUI launch, not in the exec launch of fleetd, so no session is
/// recorded.
#[test]
fn arc_26_e2e_lifecycle_matrix_antigravity() {
    lifecycle("antigravity", "antigravity", "antigravity", false);
}

/// G3: the pane shell holds the forbidden key (herdr keeps the operator's
/// environment), so the pane command removes it before horch starts. The
/// recorded line runs again in a shell that holds the key, with a probe in
/// place of horch, and the probe does not see the key.
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

    // The quoted word that is the horch binary.
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

/// F7 (G3): the worker loads the roster in its pane, and an overlay file
/// that does not load gives one `warning:` line there. The worker still
/// launches its agent.
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

/// Launch `persona` in a world with the forbidden key, and the keys in
/// `extra`, in the environment of fleetd. The key has a dummy value, and
/// no assert shows it.
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
    // The fake flags a forbidden key in its environment as a violation too.
    assert_eq!(launch["violations"], serde_json::json!([]), "{launch}");
    launch
}

/// The keys that move agy off the operator's Google login never reach an
/// agy child, even when fleetd's own environment holds them.
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

/// SR-7 for codex. The claude launch has its test in `p7_launcher.rs`
/// (`no_child_of_a_worker_job_check_docs_sync_or_conformance_has_the_forbidden_key`).
#[test]
fn codex_child_never_receives_a_forbidden_key() {
    launch_with_keys("codex-env", "codex-sol", "codex", &[]);
}

/// The model of the persona reaches the claude command line. A seat on
/// Fable launches `claude --model fable`.
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

/// The session that the ledger records is the session file that the fake
/// Prime wrote, in a directory only its launch knows.
#[test]
fn tel_session_recorded_when_prime_ends_fast() {
    let (launch, session) = lifecycle("prime", "prime", "prime", true);
    assert_eq!(session["session_id"], launch["session_file"], "{session}");
}

/// A failed pane split launches nothing: the worker is `launch_failed`, and
/// no pane runs a command.
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
