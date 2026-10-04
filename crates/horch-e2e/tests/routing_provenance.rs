//! ARC-14: every record a spawn writes says how routing chose it.

use std::process::Output;

use horch_e2e::harness::{fixtures, Harness};
use serde_json::Value;

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn quota_fixture(name: &str) -> String {
    fixtures()
        .join("quota")
        .join(format!("{name}.json"))
        .to_string_lossy()
        .into_owned()
}

fn gated(name: &str, fixture: &str) -> Harness {
    let mut h = Harness::new(name);
    h.set("HORCH_NOW", "2026-09-28T18:00:00Z");
    h.set("HORCH_PROBE_TIMEOUT_MS", "1500");
    h.set("HORCH_QUOTA_FILE", quota_fixture(fixture));
    h.set("HORCH_PROJECT_DIR", "/work/alpha");
    h.set("HORCH_WORKSPACE_ID", "w1");
    h
}

fn records(h: &Harness) -> Vec<Value> {
    let text = std::fs::read_to_string(h.state.join("-work-alpha.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn record(h: &Harness, role: &str) -> Value {
    records(h)
        .into_iter()
        .find(|r| r["role"] == role)
        .unwrap_or_else(|| panic!("no record with role {role}"))
}

fn spawn(h: &Harness, args: &[&str]) -> Output {
    let mut argv = vec!["spawn"];
    argv.extend_from_slice(args);
    argv.extend_from_slice(&["--from-pane", "w1:p1", "--no-tile"]);
    let out = h.run(&argv);
    assert!(
        out.status.success(),
        "{argv:?}\n{}\n{}",
        stdout(&out),
        stderr(&out)
    );
    out
}

#[test]
fn arc_14_provenance_on_spawn_substitute_resume() {
    let mut h = gated("arc14", "claude-exhausted-codex-ok");

    // A normal spawn: codex is ok, so codex-sol runs as itself.
    spawn(&h, &["codex-sol", "plain"]);
    let plain = record(&h, "codex-sol-1");
    let p = &plain["routing"];
    assert_eq!(p["requested"], "codex-sol", "{plain:#}");
    assert_eq!(p["resolved"], "codex-sol");
    assert_eq!(p["mode"], "auto");
    assert_eq!(p["pool"], "codex");
    assert_eq!(p["pool_state"], "ok");
    assert!(p["fallback_index"].is_null());
    assert!(plain.get("via").is_none(), "no via on a plain spawn");

    // A substituted spawn: claude is exhausted, so researcher runs on its
    // fallback. `via` and `substitution_reason` are still written.
    let out = spawn(&h, &["researcher", "sub"]);
    assert!(stdout(&out).starts_with("SUBSTITUTED: researcher runs on codex-sol."));
    let sub = record(&h, "researcher-1");
    let p = &sub["routing"];
    assert_eq!(p["requested"], "researcher", "{sub:#}");
    assert_eq!(p["resolved"], "codex-sol");
    assert_eq!(p["mode"], "auto");
    assert_eq!(p["pool"], "codex");
    assert_eq!(p["pool_state"], "ok");
    assert_eq!(p["reason"], "claude 7d 100%, resets 2026-10-02T14:00Z");
    assert!(p["fallback_index"].is_u64());
    assert_eq!(sub["via"], "codex-sol");
    assert_eq!(
        sub["substitution_reason"],
        "claude 7d 100%, resets 2026-10-02T14:00Z"
    );

    // A resume keeps the record's routing, with mode `resume`, even when the
    // pools have changed since.
    let id = sub["record_id"].as_str().unwrap().to_string();
    assert!(h
        .run(&[
            "ledger",
            "set-session",
            &id,
            "01d0e61c-d73e-74a3-837c-b5aade8b1c38"
        ])
        .status
        .success());
    assert!(h.run(&["ledger", "done", &id, "finished"]).status.success());
    h.set("HORCH_QUOTA_FILE", quota_fixture("all-ok"));
    spawn(&h, &["--resume", &id, "more", "--role", "researcher-9"]);
    let resumed = record(&h, "researcher-9");
    assert_eq!(resumed["record_id"], sub["record_id"]);
    let p = &resumed["routing"];
    assert_eq!(p["requested"], "researcher", "{resumed:#}");
    assert_eq!(p["resolved"], "codex-sol");
    assert_eq!(p["mode"], "resume");
    assert_eq!(p["pool_state"], sub["routing"]["pool_state"]);
    assert_eq!(p["fallback_index"], sub["routing"]["fallback_index"]);
    assert_eq!(resumed["via"], "codex-sol");
}
