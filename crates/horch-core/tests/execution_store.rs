//! The execution store and the ledger facade over it (A6: ARC-17, SKL-04).

use std::time::Duration;

use horch_core::ledger::{Ledger, Record, KIND_WORKER, STATUS_WORKING};
use horch_core::teammates::Phase;

fn ledger() -> (tempfile::TempDir, Ledger) {
    let tmp = tempfile::tempdir().unwrap();
    let l = Ledger::for_project(tmp.path(), "/Users/a/proj");
    (tmp, l)
}

// ─── ledger facade tests that touch the file directly ───────────────────────
//
// Moved from `ledger.rs` unchanged when it became a facade with no file I/O.

#[test]
fn effort_is_recorded_rendered_and_optional_on_disk() {
    let (_t, l) = ledger();
    l.add("r1", "claude", "opus", "opus", "opus-1", Some("s1"), "t")
        .unwrap();
    assert_eq!(l.get("r1").unwrap().effort, None);
    assert!(!std::fs::read_to_string(l.path())
        .unwrap()
        .contains("effort"));
    l.set_effort("s1", Some("medium")).unwrap();
    assert_eq!(l.get("r1").unwrap().effort.as_deref(), Some("medium"));
    assert!(l.render().unwrap().contains("model=opus effort=medium"));
    l.set_effort("r1", None).unwrap();
    assert_eq!(l.get("r1").unwrap().effort, None);
    assert!(l.set_effort("missing", Some("low")).is_err());
}

/// A killed pane can leave the lock dir behind; the ledger must recover
/// rather than hang forever.
#[test]
fn a_stale_lock_is_broken_rather_than_deadlocking() {
    let (_t, l) = ledger();
    std::fs::create_dir_all(l.path().parent().unwrap()).unwrap();
    std::fs::create_dir(l.path().with_extension("json.lock")).unwrap();

    let started = std::time::Instant::now();
    l.add(
        "r1",
        "claude",
        "sonnet",
        "sonnet",
        "sonnet-1",
        Some("s1"),
        "t",
    )
    .unwrap();
    assert!(l.get("r1").is_ok());
    // ~15s of spinning before the break, then success.
    assert!(started.elapsed() < Duration::from_secs(45));
}

/// The identity fields default in, round-trip, and stay off disk when
/// unset, so a worker record written today reads like one from before.
#[test]
fn tel_08_insert_fills_plan_project_and_workspace() {
    let (_t, l) = ledger();
    let l = l.with_workspace(Some("w7".into()));
    l.insert(Record {
        record_id: "r1".into(),
        session_id: Some("s1".into()),
        agent: "claude".into(),
        tier: "sonnet".into(),
        model: "sonnet".into(),
        role: "sonnet-1".into(),
        project: Some("/work/alpha".into()),
        task: "Do ai_docs/plans/golden-prompts-whitespace.md step 2".into(),
        phase: Some(Phase::Implementation),
        ..Record::default()
    })
    .unwrap();
    let r = l.get("r1").unwrap();
    assert_eq!(r.kind, KIND_WORKER);
    assert_eq!(r.plan.as_deref(), Some("golden-prompts-whitespace"));
    assert_eq!(r.project.as_deref(), Some("/work/alpha"));
    assert_eq!(r.workspace_id.as_deref(), Some("w7"));
    assert_eq!(r.status, STATUS_WORKING);
    let raw = std::fs::read_to_string(l.path()).unwrap();
    assert!(
        !raw.contains("\"kind\""),
        "a worker's kind stays implicit: {raw}"
    );
    assert!(!raw.contains("\"via\""), "{raw}");

    l.assign("sonnet-1", "now ai_docs/plans/other.md").unwrap();
    assert_eq!(l.get("r1").unwrap().plan.as_deref(), Some("other"));
    l.assign("sonnet-1", "no plan").unwrap();
    assert_eq!(l.get("r1").unwrap().plan, None);
}

/// Records written by the bash implementation must still load.
#[test]
fn reads_a_ledger_written_by_the_bash_version() {
    let tmp = tempfile::tempdir().unwrap();
    let l = Ledger::for_project(tmp.path(), "/p");
    std::fs::create_dir_all(tmp.path()).unwrap();
    std::fs::write(
        l.path(),
        r#"[
          {"record_id":"3f2b","session_id":null,"agent":"codex","tier":"codex-sol",
           "model":"gpt-5.6-sol","role":"codex-sol-1","status":"working",
           "task":"(idle - awaiting assignment)",
           "history":[{"at":"2026-07-08T10:00:00Z","event":"spawned",
                       "text":"spawned idle as codex-sol-1"}],
           "created_at":"2026-07-08T10:00:00Z","updated_at":"2026-07-08T10:00:00Z"}
        ]"#,
    )
    .unwrap();

    let r = l.get("3f2b").unwrap();
    assert_eq!(r.role, "codex-sol-1");
    assert_eq!(r.phase, None);
    assert_eq!(r.session_id, None);
    // And it stays writable.
    l.note("3f2b", "still going").unwrap();
    assert_eq!(l.get("3f2b").unwrap().history.len(), 2);
}
