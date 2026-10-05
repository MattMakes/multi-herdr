//! End-to-end: a worker's whole life through the real `horch` binary.
//!
//! - ARC-26: for each of the 6 harnesses, a fresh spawn, the session found
//!   (minted by horch or discovered from the harness), `horch done`, then
//!   `horch spawn --resume` reusing that session.
//! - ARC-16: a failed pane split and a crash after the insert leave no
//!   live-looking record.
//!
//! fake-herdr runs in the `exec` scenario, so `horch spawn`'s pane command
//! really starts `horch worker`, and `pane close` kills it. The harness fakes
//! run in `stay`, so an agent lives until its pane closes.

use std::process::Output;
use std::time::{Duration, Instant};

use horch_e2e::harness::{fixtures, repo_root, Harness};
use serde_json::Value;

const NOW: &str = "2026-09-28T18:00:00Z";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A harness with workspace `w1` and its root pane `w1:p1`, as `horch fleet`
/// leaves it.
fn fleet(name: &str, scenario: &str) -> Harness {
    let mut h = Harness::new(name);
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_NOW", NOW);
    h.set("HORCH_FAKE_SCENARIO", scenario);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let out = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    h
}

fn records(h: &Harness) -> Vec<Value> {
    let slug: String = h
        .project
        .to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect();
    std::fs::read_to_string(h.state.join(format!("{slug}.json")))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn record(h: &Harness, id: &str) -> Value {
    records(h)
        .into_iter()
        .find(|r| r["record_id"] == id)
        .unwrap_or_else(|| panic!("no record {id}"))
}

fn wait_for<T>(what: &str, f: impl FnMut() -> Option<T>) -> T {
    wait_for_or(what, f, String::new)
}

/// [`wait_for`], with `context` in the panic message.
fn wait_for_or<T>(what: &str, mut f: impl FnMut() -> Option<T>, context: impl Fn() -> String) -> T {
    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}\n{}", context());
}

/// What a worker pane wrote (fake-herdr `exec` saves its output), and the
/// record, for a failure message.
fn pane_context(h: &Harness, pane: &str, id: &str) -> String {
    let mut out = h.log.clone().into_os_string();
    out.push(format!(".pane-{}.out", pane.replace(':', "_")));
    format!(
        "record: {}\npane output:\n{}",
        record(h, id),
        std::fs::read_to_string(&out).unwrap_or_else(|e| format!("({e})"))
    )
}

/// `horch spawn`, returning the new pane id (the last stdout line).
fn spawn(h: &Harness, args: &[&str]) -> String {
    let mut argv = vec!["spawn"];
    argv.extend_from_slice(args);
    argv.extend_from_slice(&["--from-pane", "w1:p1", "--no-tile"]);
    let out = h.run(&argv);
    assert!(out.status.success(), "{}", text(&out));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_string()
}

/// `horch done` as the agent in `pane` runs it: the transport variables say
/// who it is. Closing the pane kills the worker and its agent.
fn done(h: &Harness, pane: &str, role: &str, id: &str) {
    let mut cmd = h.horch(&["done", "finished the work"]);
    cmd.env("HERDR_PANE_ID", pane)
        .env("HORCH_ROLE", role)
        .env("HORCH_RECORD_ID", id);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
}

/// The last launch of `fake` whose argv names `needle`.
fn launch_naming(h: &Harness, fake: &str, needle: &str) -> Option<Value> {
    h.calls_of(fake).into_iter().rev().find(|c| {
        c["argv"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(needle)))
    })
}

/// Fresh spawn → session known → done → resume on the same session.
fn lifecycle(harness: &str, teammate: &str, fake: &str) {
    let h = fleet(&format!("arc26-{harness}"), "exec,stay");
    lifecycle_in(h, harness, teammate, fake);
}

/// [`lifecycle`] in a harness the caller has set up.
fn lifecycle_in(h: Harness, harness: &str, teammate: &str, fake: &str) {
    let pane = spawn(&h, &[teammate, "build the thing"]);
    let fresh = records(&h)
        .into_iter()
        .find(|r| r["tier"] == teammate)
        .expect("the spawn wrote a record");
    let id = fresh["record_id"].as_str().unwrap().to_string();
    let role = fresh["role"].as_str().unwrap().to_string();
    assert_eq!(fresh["agent"], harness);

    // The worker marks itself running, and the session id is known: minted
    // by horch (claude, pi) or discovered while the agent runs. The claude
    // and pi fakes exit at once, so their worker may already be done.
    let session = wait_for_or(
        "the session id",
        || {
            let r = record(&h, &id);
            let running = ["running", "done"].contains(&r["state"]["state"].as_str().unwrap_or(""));
            let session = r["session_id"].as_str().filter(|s| !s.is_empty());
            session.filter(|_| running).map(str::to_owned)
        },
        || pane_context(&h, &pane, &id),
    );
    assert_eq!(record(&h, &id)["pane_id"], pane.as_str());

    done(&h, &pane, &role, &id);
    let r = record(&h, &id);
    assert_eq!(r["status"], "done", "{r}");
    assert_eq!(r["state"]["state"], "done", "{r}");

    let pane2 = spawn(&h, &["--resume", &id, "keep going"]);
    let resumed = wait_for("the resumed launch", || launch_naming(&h, fake, &session));
    assert!(
        resumed["violations"].as_array().unwrap().is_empty(),
        "{resumed}"
    );
    let r = record(&h, &id);
    assert_eq!(r["session_id"], session.as_str(), "the session is reused");
    assert_eq!(
        r["history"].as_array().unwrap().last().unwrap()["event"],
        "resumed"
    );
    assert_eq!(r["routing"]["mode"], "resume");
    assert_eq!(records(&h).len(), 1, "a resume writes no new record");

    // Close the resumed pane too, which stops its agent.
    let role2 = r["role"].as_str().unwrap().to_string();
    done(&h, &pane2, &role2, &id);
}

#[test]
fn arc_26_e2e_lifecycle_matrix_claude() {
    lifecycle("claude", "sonnet", "claude");
}

#[test]
fn arc_26_e2e_lifecycle_matrix_codex() {
    lifecycle("codex", "codex-sol", "codex");
}

#[test]
fn arc_26_e2e_lifecycle_matrix_opencode() {
    lifecycle("opencode", "opencode-pickle", "opencode");
}

#[test]
fn arc_26_e2e_lifecycle_matrix_pi() {
    lifecycle("pi", "pi", "pi");
}

#[test]
fn arc_26_e2e_lifecycle_matrix_prime() {
    lifecycle("prime", "prime", "prime");
}

/// `fake-antigravity` as `agy` (the harness installs it in `bin/`), named by
/// `HORCH_ANTIGRAVITY_BIN`.
fn with_agy(h: &mut Harness) {
    let name = format!("agy{}", std::env::consts::EXE_SUFFIX);
    let bin = h.bin.join(name).to_string_lossy().into_owned();
    h.set("HORCH_ANTIGRAVITY_BIN", bin);
}

/// agy mints its own conversation id; horch reads it back from the session
/// cache and resumes it with `--conversation`.
#[test]
fn arc_26_e2e_lifecycle_matrix_antigravity() {
    let mut h = fleet("arc26-antigravity", "exec,stay");
    with_agy(&mut h);
    lifecycle_in(h, "antigravity", "antigravity", "antigravity");
}

/// G3: the pane shell holds `ANTHROPIC_API_KEY` (herdr keeps the operator's
/// environment), so the pane command removes it before horch starts. The
/// recorded line runs again in a shell that holds the key, with a probe in
/// place of horch, and the probe does not see the key.
#[cfg(unix)]
#[test]
fn pane_command_removes_a_forbidden_key_before_horch_starts() {
    let mut h = fleet("pane-env", "default");
    h.set("ANTHROPIC_API_KEY", "SENTINEL");
    let pane = spawn(&h, &["sonnet", "build the thing"]);
    let ran = h
        .calls_of("herdr")
        .into_iter()
        .find_map(|c| c["ran"].as_str().map(str::to_owned))
        .expect("horch spawn ran a pane command");
    assert!(
        ran.starts_with("/usr/bin/env -u ANTHROPIC_API_KEY '"),
        "{pane}: {ran}"
    );

    // The first quoted word is the horch binary.
    let start = ran.find('\'').unwrap();
    let end = start + 1 + ran[start + 1..].find('\'').unwrap();
    let horch = &ran[start..=end];
    let out_file = h.tmp.join("pane-probe.env");
    let probe = h.write_bin(
        "pane-probe",
        b"#!/bin/sh\n/usr/bin/env > \"$PANE_PROBE_OUT\"\n",
    );
    let line = ran.replacen(horch, &format!("'{}'", probe.display()), 1);
    let mut shell = std::process::Command::new("/bin/sh");
    h.seal(&mut shell);
    shell.arg("-c").arg(&line).env("PANE_PROBE_OUT", &out_file);
    let out = shell.output().unwrap();
    assert!(out.status.success(), "{line}\n{}", text(&out));
    let env = std::fs::read_to_string(&out_file).unwrap();
    assert!(
        env.lines().any(|l| l.starts_with("HORCH_FAKE_LOG=")),
        "{env}"
    );
    assert!(
        !env.lines().any(|l| l.starts_with("ANTHROPIC_API_KEY=")),
        "{env}"
    );
}

/// F7 (G3): the worker loads the roster in its pane, and an overlay file
/// that does not load gives one `warning:` line there. The worker still
/// launches its agent.
#[test]
fn worker_warns_once_about_a_teammate_file_that_does_not_load() {
    let h = fleet("f7-worker", "default");
    let dir = h.home.join(".config/horch/teammates");
    std::fs::create_dir_all(&dir).unwrap();
    let broken = std::fs::read_to_string(repo_root().join("teammates/opus.md"))
        .unwrap()
        .replace("name: opus", "name: newcomer")
        .replacen("\n---\n", "\nrequires: [no-such-tool]\n---\n", 1);
    std::fs::write(dir.join("newcomer.md"), broken).unwrap();
    let pane = spawn(&h, &["sonnet", "build the thing"]);
    let role = records(&h)[0]["role"].as_str().unwrap().to_string();
    let mut worker = h.horch(&["worker", &role]);
    worker.env("HERDR_PANE_ID", &pane);
    let out = worker.output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|l| l.starts_with("warning: teammate 'newcomer'"))
        .collect();
    assert_eq!(warnings.len(), 1, "{stderr}");
    assert!(
        warnings[0].contains("unknown variant `no-such-tool`"),
        "{stderr}"
    );
    assert_eq!(
        h.calls_of("claude").len(),
        1,
        "the worker launched its agent"
    );
}

/// The keys that move agy off the operator's Google login never reach an
/// agy child, even when the worker's own environment holds them.
#[test]
fn antigravity_child_never_receives_a_forbidden_key() {
    const KEYS: [&str; 4] = [
        "ANTHROPIC_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GOOGLE_GEMINI_BASE_URL",
    ];
    let mut h = fleet("agy-env", "default");
    with_agy(&mut h);
    let pane = spawn(&h, &["antigravity", "build the thing"]);
    let role = records(&h)[0]["role"].as_str().unwrap().to_string();
    let mut worker = h.horch(&["worker", &role]);
    worker.env("HERDR_PANE_ID", &pane);
    for key in KEYS {
        worker.env(key, "must-not-leak");
    }
    let out = worker.output().unwrap();
    assert!(out.status.success(), "{}", text(&out));

    let agy = h
        .calls_of("antigravity")
        .into_iter()
        .last()
        .expect("the worker launched agy");
    let keys: Vec<String> = serde_json::from_value(agy["env_keys"].clone()).unwrap();
    for key in KEYS {
        assert!(
            !keys.iter().any(|k| k == key),
            "{key} reached agy: {keys:?}"
        );
    }
    // fake-antigravity flags each of them as a violation too.
    assert_eq!(agy["violations"], serde_json::json!([]), "{agy}");
    let argv: Vec<String> = serde_json::from_value(agy["argv"].clone()).unwrap();
    assert_eq!(&argv[..2], ["--model", "gemini-3.8-flash"], "{argv:?}");
    assert!(
        argv.windows(2).any(|w| w == ["--effort", "medium"]),
        "{argv:?}"
    );
    assert_eq!(argv[argv.len() - 2], "--prompt-interactive", "{argv:?}");
}

// ─── ARC-16 ─────────────────────────────────────────────────────────────────

/// An agent that ends within 1 s of launch still gets its session id
/// recorded: `horch done` runs one last discovery before it closes the pane.
#[test]
fn tel_session_recorded_when_agent_ends_fast() {
    let h = fleet("tel-fast-end", "exec,stay");
    let pane = spawn(&h, &["codex-sol", "build the thing"]);
    let fresh = records(&h)
        .into_iter()
        .find(|r| r["tier"] == "codex-sol")
        .expect("the spawn wrote a record");
    let id = fresh["record_id"].as_str().unwrap().to_string();
    let role = fresh["role"].as_str().unwrap().to_string();

    // The fake has written its rollout; `done` follows at once.
    let minted = wait_for("the fake's session id", || {
        h.calls_of("codex")
            .iter()
            .find_map(|c| c["session_id"].as_str().map(str::to_owned))
    });
    done(&h, &pane, &role, &id);

    let r = record(&h, &id);
    assert_eq!(r["status"], "done", "{r}");
    assert_eq!(r["session_id"], minted.as_str(), "{r}");
}

/// The same for Prime, whose session file is in a directory only its launch
/// knows: the launch marker carries that directory to `horch done`.
#[test]
fn tel_session_recorded_when_prime_ends_fast() {
    let h = fleet("tel-fast-end-prime", "exec,stay");
    let pane = spawn(&h, &["prime", "build the thing"]);
    let fresh = records(&h)
        .into_iter()
        .find(|r| r["tier"] == "prime")
        .expect("the spawn wrote a record");
    let id = fresh["record_id"].as_str().unwrap().to_string();
    let role = fresh["role"].as_str().unwrap().to_string();

    // The fake has written its session file; `done` follows at once.
    let file = wait_for("the fake's session file", || {
        h.calls_of("prime")
            .iter()
            .find_map(|c| c["session_file"].as_str().map(str::to_owned))
    });
    done(&h, &pane, &role, &id);

    let r = record(&h, &id);
    assert_eq!(r["status"], "done", "{r}");
    assert_eq!(r["session_id"], file.as_str(), "{r}");
}

#[test]
fn arc_16_e2e_fail_split() {
    let h = fleet("arc16-split", "fail_split");
    let out = h.run(&["spawn", "sonnet", "x", "--from-pane", "w1:p1", "--no-tile"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert_ne!(
        out.status.code(),
        Some(3),
        "a launch failure is not a refusal"
    );
    let all = records(&h);
    assert_eq!(all.len(), 1, "{all:?}");
    let r = &all[0];
    assert_eq!(r["state"]["state"], "launch_failed", "{r}");
    assert_eq!(r["state"]["stage"], "split", "{r}");
    assert_eq!(r["status"], "done", "an old reader sees it finished");
    assert!(r["finished_at"].is_string());
    let role = r["role"].as_str().unwrap();
    assert!(!h
        .tmp
        .join("herdr-orchestration-w1")
        .join(format!("{role}.brief.json"))
        .exists());
    let sessions = h.run(&["sessions", "--json"]);
    assert!(sessions.status.success(), "{}", text(&sessions));
}

/// A crash right after the insert: the record is `Planned`, which is never
/// live. The recovery rule closes it on a spawn 5 minutes or more later.
#[test]
fn arc_16_crash_after_insert_not_live() {
    let mut h = fleet("arc16-crash", "default");
    h.set("HORCH_FAULT", "abort-after-execution-insert");
    let out = h.run(&["spawn", "sonnet", "x", "--from-pane", "w1:p1", "--no-tile"]);
    assert_eq!(out.status.code(), Some(86), "{}", text(&out));
    let r = records(&h).pop().unwrap();
    assert_eq!(r["state"]["state"], "planned", "{r}");
    assert!(r.get("pane_id").is_none());
    assert!(
        !h.calls_of("herdr").iter().any(|c| c["argv"][1] == "split"),
        "no pane split"
    );
    let crashed = r["record_id"].as_str().unwrap().to_string();

    // A later spawn recovers it and goes on.
    h.unset("HORCH_FAULT");
    h.set("HORCH_NOW", "2026-09-28T18:05:00Z");
    spawn(&h, &["sonnet", "y"]);
    let r = record(&h, &crashed);
    assert_eq!(r["state"]["state"], "launch_failed", "{r}");
    assert_eq!(r["status"], "done");
    let live: Vec<Value> = records(&h)
        .into_iter()
        .filter(|r| r["state"]["state"] == "starting")
        .collect();
    assert_eq!(live.len(), 1, "only the new spawn is live");
}
