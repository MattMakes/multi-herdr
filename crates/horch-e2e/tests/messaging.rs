//! End-to-end (G5): fleet messaging through the real `horch` binary, in a
//! workspace with no registered orchestrator.
//!
//! - `horch done` records the summary, tells nobody, says why, exits 0.
//! - `horch inbox` marks a role whose pane closed without `horch done`.
//! - `horch spawn --resume` of that record ends it and resumes it; while the
//!   pane is open, it refuses and names the command that fixes it.
//!
//! fake-herdr runs in the `exec` scenario, so a pane runs `horch worker`, and
//! `pane close` kills it. The codex fake runs in `stay`.

use std::process::Output;
use std::time::{Duration, Instant};

use horch_e2e::harness::{fixtures, Harness};
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

/// Poll `f` for 30 s, with `context` in the panic message.
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

/// A fake-herdr call, as the operator would make it by hand.
fn herdr(h: &Harness, args: &[&str]) -> Output {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    cmd.args(args).output().unwrap()
}

/// `horch` as the worker in `pane` runs it.
fn as_worker(h: &Harness, args: &[&str], pane: &str, role: &str, id: &str) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HERDR_PANE_ID", pane)
        .env("HORCH_ROLE", role)
        .env("HORCH_RECORD_ID", id);
    cmd.output().unwrap()
}

/// A spawned codex worker that is running, as `(pane, role, record id)`.
fn running_worker(h: &Harness) -> (String, String, String) {
    let pane = spawn(h, &["codex-sol", "build the thing"]);
    let r = records(h).pop().expect("the spawn wrote a record");
    let id = r["record_id"].as_str().unwrap().to_string();
    let role = r["role"].as_str().unwrap().to_string();
    wait_for_or(
        "the running worker",
        || {
            let r = record(h, &id);
            (r["state"]["state"] == "running" && r["session_id"].is_string()).then_some(())
        },
        || pane_context(h, &pane, &id),
    );
    (pane, role, id)
}

/// G5: in a workspace with no registered orchestrator, a worker's `tell` to
/// it fails and says that `horch done` still works; `done` records the
/// summary, tells nobody, says why, and exits 0.
#[test]
fn g5_done_without_an_orchestrator() {
    let h = fleet("g5-done", "exec,stay");
    let (pane, role, id) = running_worker(&h);

    let tell = as_worker(&h, &["tell", "orchestrator", "DONE: x"], &pane, &role, &id);
    assert!(!tell.status.success(), "{}", text(&tell));
    assert!(
        text(&tell).contains("still records your summary"),
        "{}",
        text(&tell)
    );

    let done = as_worker(&h, &["done", "finished the work"], &pane, &role, &id);
    assert!(done.status.success(), "{}", text(&done));
    assert!(
        text(&done).contains("no orchestrator is registered in this workspace"),
        "{}",
        text(&done)
    );
    let r = record(&h, &id);
    assert_eq!(r["status"], "done", "{r}");
}

/// G5: a worker pane closed without `horch done`. `horch inbox` marks the
/// role, and `horch spawn --resume` ends the record ("pane closed without
/// horch done") and resumes it. While the pane is open, the resume refuses.
#[test]
fn g5_resume_after_a_closed_pane() {
    let h = fleet("g5-resume", "exec,stay");
    let (pane, role, id) = running_worker(&h);

    let out = h.run(&[
        "spawn",
        "--resume",
        &id,
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ]);
    assert!(!out.status.success(), "the pane is open: {}", text(&out));
    assert!(
        text(&out).contains(&format!("horch ledger done {id}")),
        "{}",
        text(&out)
    );

    let closed = herdr(&h, &["pane", "close", &pane]);
    assert!(closed.status.success(), "{}", text(&closed));
    assert_eq!(record(&h, &id)["status"], "working", "no horch done ran");

    let inbox = h.run(&["inbox"]);
    assert!(inbox.status.success(), "{}", text(&inbox));
    let stdout = String::from_utf8_lossy(&inbox.stdout);
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&role))
        .unwrap_or_else(|| panic!("no {role} in {stdout}"));
    assert!(line.ends_with("(pane closed)"), "{stdout}");

    let pane2 = spawn(&h, &["--resume", &id, "keep going"]);
    let r = record(&h, &id);
    let events: Vec<(&str, &str)> = r["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["event"].as_str().unwrap(),
                e["text"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert!(
        events.contains(&("ended", "pane closed without horch done")),
        "{r}"
    );
    assert_eq!(events.last().unwrap().0, "resumed", "{r}");

    let role2 = r["role"].as_str().unwrap().to_string();
    done(&h, &pane2, &role2, &id);
}
