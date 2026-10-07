//! End-to-end: the fleet run facts hooks of `horch spawn` and `horch done`
//! (`docs/specs/fleet-dataset.md` §4, FDS-02, FDS-04, FDS-06).
//!
//! fake-herdr runs in the `exec` scenario, so `horch spawn`'s pane command
//! really starts `horch worker`, as in `lifecycle.rs`.

use std::path::PathBuf;
use std::process::Output;

use horch_e2e::harness::{fixtures, wait_for, Harness};
use serde_json::{json, Value};

const NOW: &str = "2026-09-28T18:00:00Z";
const NOTE_RUN: &str = "horch: NOTE: fleet run row not written:";
const NOTE_START: &str = "horch: NOTE: fleet start row not written:";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A harness with workspace `w1` and its root pane `w1:p1`.
fn fleet(name: &str) -> Harness {
    let mut h = Harness::new(name);
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_NOW", NOW);
    h.set("HORCH_FAKE_SCENARIO", "exec,stay");
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let out = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    h
}

fn slug(h: &Harness) -> String {
    h.project
        .to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect()
}

fn ledger_path(h: &Harness) -> PathBuf {
    h.state.join(format!("{}.json", slug(h)))
}

fn fleet_dir(h: &Harness) -> PathBuf {
    h.state.join("multi-herdr").join(slug(h)).join("fleet")
}

fn records(h: &Harness) -> Vec<Value> {
    std::fs::read_to_string(ledger_path(h))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn rows(h: &Harness, file: &str) -> Vec<Value> {
    std::fs::read_to_string(fleet_dir(h).join(file))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// `horch spawn`; the output, which must succeed.
fn spawn(h: &Harness, args: &[&str]) -> Output {
    let mut argv = vec!["spawn"];
    argv.extend_from_slice(args);
    argv.extend_from_slice(&["--from-pane", "w1:p1", "--no-tile"]);
    let out = h.run(&argv);
    assert!(out.status.success(), "{}", text(&out));
    out
}

fn pane_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_string()
}

/// `horch done` as the agent in `pane` runs it; the output, which must
/// succeed.
fn done(h: &Harness, pane: &str, role: &str, id: &str) -> Output {
    let mut cmd = h.horch(&["done", "finished the work"]);
    cmd.env("HERDR_PANE_ID", pane)
        .env("HORCH_ROLE", role)
        .env("HORCH_RECORD_ID", id);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    out
}

/// Spawn 1 claude worker with a plan file task; its pane, record id and
/// role, once it runs.
fn spawn_worker(h: &Harness) -> (Output, String, String, String) {
    std::fs::create_dir_all(h.project.join("ai_docs/plans")).unwrap();
    std::fs::write(h.project.join("ai_docs/plans/p.md"), "the plan").unwrap();
    let out = spawn(
        h,
        &["sonnet", "Read and follow ai_docs/plans/p.md exactly."],
    );
    let pane = pane_of(&out);
    let r = records(h)
        .into_iter()
        .find(|r| r["tier"] == "sonnet")
        .expect("the spawn wrote a record");
    let id = r["record_id"].as_str().unwrap().to_string();
    let role = r["role"].as_str().unwrap().to_string();
    wait_for("the worker to run", || {
        let r = records(h).into_iter().find(|r| r["record_id"] == id)?;
        ["running", "done"]
            .contains(&r["state"]["state"].as_str().unwrap_or(""))
            .then_some(())
    });
    (out, pane, id, role)
}

fn note_lines(out: &Output, prefix: &str) -> usize {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter(|l| l.starts_with(prefix))
        .count()
}

/// FDS-04: `horch done` writes its record's run row, with the start row's
/// plan facts; FDS-02: the spawn wrote that start row.
#[test]
fn fds_04_done_writes_run_row() {
    let h = fleet("fds04-done");
    let (out, pane, id, role) = spawn_worker(&h);
    assert_eq!(note_lines(&out, "horch: NOTE: fleet"), 0, "{}", text(&out));
    let starts = rows(&h, "starts.jsonl");
    assert_eq!(starts.len(), 1, "{starts:?}");
    assert_eq!(starts[0]["record_id"], id.as_str());
    assert_eq!(starts[0]["plan_path"], "ai_docs/plans/p.md");
    assert_eq!(starts[0]["plan_text"], "the plan");
    assert!(
        rows(&h, "runs.jsonl").is_empty(),
        "a running worker has no row"
    );

    let out = done(&h, &pane, &role, &id);
    assert_eq!(note_lines(&out, NOTE_RUN), 0, "{}", text(&out));
    let runs = rows(&h, "runs.jsonl");
    assert_eq!(runs.len(), 1, "{runs:?}");
    let run = &runs[0];
    assert_eq!(run["schema"], "mh.fleet-run/1.0.0");
    assert_eq!(run["record_id"], id.as_str());
    assert_eq!(run["role"], role.as_str());
    assert_eq!(run["teammate"], "sonnet");
    assert_eq!(run["kind"], "worker");
    assert_eq!(run["written_by"], "done");
    assert_eq!(run["done_summary"], "finished the work");
    assert_eq!(run["plan_path"], "ai_docs/plans/p.md");
    assert_eq!(run["end"]["status"], "done");
    assert!(run.get("plan_text").is_none(), "no plan text in a run row");

    // The next spawn's sync finds the row there and writes no second one.
    let out = spawn(&h, &["sonnet", "more work"]);
    assert_eq!(note_lines(&out, "horch: NOTE: fleet"), 0, "{}", text(&out));
    assert_eq!(rows(&h, "runs.jsonl").len(), 1);
    assert_eq!(rows(&h, "starts.jsonl").len(), 2);
}

/// FDS-04: a fleet dir that cannot be written gives 1 NOTE line, and
/// `horch done` still marks the record done and exits 0.
#[cfg(unix)]
#[test]
fn fds_04_done_write_failure_is_a_note() {
    use std::os::unix::fs::PermissionsExt;
    let h = fleet("fds04-fail");
    let (_, pane, id, role) = spawn_worker(&h);
    let dir = fleet_dir(&h);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let mut cmd = h.horch(&["done", "finished the work"]);
    cmd.env("HERDR_PANE_ID", &pane)
        .env("HORCH_ROLE", &role)
        .env("HORCH_RECORD_ID", &id);
    let out = cmd.output().unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(note_lines(&out, NOTE_RUN), 1, "{}", text(&out));
    assert!(
        String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "{}",
        text(&out)
    );
    let r = records(&h)
        .into_iter()
        .find(|r| r["record_id"] == id)
        .unwrap();
    assert_eq!(r["status"], "done", "{r}");
    assert!(rows(&h, "runs.jsonl").is_empty());
}

/// FDS-02: a start row that cannot be written gives a NOTE line; the spawn
/// succeeds and prints only its pane id on stdout.
#[test]
fn fds_02_start_row_failure_is_a_note() {
    let h = fleet("fds02-fail");
    // A file where the fleet dir belongs.
    let dir = fleet_dir(&h);
    std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
    std::fs::write(&dir, "not a directory").unwrap();
    let out = spawn(&h, &["sonnet", "build the thing"]);
    assert_eq!(note_lines(&out, NOTE_START), 1, "{}", text(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.lines().count(), 1, "{}", text(&out));
    assert!(stdout.trim().starts_with("w1:"), "{}", text(&out));
    assert_eq!(records(&h).len(), 1);
}

/// FDS-06: `horch spawn` syncs: a finished record without a row (its pane
/// closed without `horch done`) gets a `sync` row; a candidate gets none.
#[test]
fn fds_06_spawn_runs_sync() {
    let h = fleet("fds06-sync");
    let finished = |id: &str, extra: Value| {
        let mut r = json!({
            "record_id": id, "session_id": null, "agent": "claude", "tier": "sonnet",
            "model": "sonnet", "role": format!("{id}-role"), "status": "done",
            "task": "old work", "history": [],
            "created_at": "2026-09-28T17:00:00Z", "updated_at": "2026-09-28T17:30:00Z",
            "state": {"state": "failed", "failure": {"kind": "pane_vanished"}},
            "finished_at": "2026-09-28T17:30:00Z",
        });
        r.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        r
    };
    let ledger = json!([
        finished("r-gone", json!({})),
        finished(
            "r-cand",
            json!({"experiment_id": "x", "round_id": "r", "label": "A"})
        ),
    ]);
    std::fs::write(
        ledger_path(&h),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();

    let out = spawn(&h, &["sonnet", "new work"]);
    assert_eq!(note_lines(&out, "horch: NOTE: fleet"), 0, "{}", text(&out));
    let runs = rows(&h, "runs.jsonl");
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0]["record_id"], "r-gone");
    assert_eq!(runs[0]["written_by"], "sync");
    assert_eq!(runs[0]["tokens"], Value::Null);
    assert_eq!(runs[0]["cost_source"], "unpriced");
    assert_eq!(runs[0]["duration_ms"], 1_800_000);
}
