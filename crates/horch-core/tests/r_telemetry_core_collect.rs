//! Collector branches the other collector tests leave out: the quota file
//! override, a cursor whose input is gone, and which records show as live.

mod common;

use common::*;
use horch_core::clock;
use horch_core::routing::quota::{PoolReading, QuotaFile, Window, POOL_CLAUDE};
use horch_core::telemetry::collect::{Collector, Probing, TIDY_EVERY_MIN};

fn now() -> chrono::DateTime<chrono::Utc> {
    clock::parse("2026-09-28T18:00:00Z").unwrap()
}

fn collector(w: &World) -> Collector {
    Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap()
}

/// A ledger of records `(name, kind, status, task)`, all updated at `now`.
fn ledger(w: &World, file: &str, records: &[(&str, &str, &str, &str)]) {
    let rows: Vec<_> = records
        .iter()
        .map(|(name, kind, status, task)| {
            serde_json::json!({
                "record_id": format!("rec-{name}"), "agent": "claude", "tier": "sonnet",
                "model": "claude-sonnet-5", "role": name, "kind": kind, "status": status,
                "task": task, "history": [], "created_at": "2026-09-28T17:00:00Z",
                "updated_at": "2026-09-28T17:59:00Z", "project": "/p"
            })
        })
        .collect();
    std::fs::write(
        w.state.join(file),
        serde_json::Value::Array(rows).to_string(),
    )
    .unwrap();
}

#[test]
fn a_quota_file_override_is_read_and_never_written() {
    let w = world(Part::Whole);
    let over = w.state.join("override-quota.json");
    let mut file = QuotaFile::default();
    file.pools.insert(
        POOL_CLAUDE.to_string(),
        PoolReading {
            observed_at: Some("2026-09-28T18:00:00Z".into()),
            windows: vec![Window {
                name: "7d".into(),
                minutes: 10080,
                scope_model: None,
                used: 0.25,
                resets_at: None,
            }],
            ..PoolReading::default()
        },
    );
    let text = serde_json::to_string(&file).unwrap();
    std::fs::write(&over, &text).unwrap();
    let mut c = collector(&w);
    c.quota_env.quota_file = Some(over.clone());
    let snap = c.tick(now()).unwrap();
    // The pool the override holds is the pool the snapshot shows.
    let shown: Vec<_> = snap
        .pools
        .iter()
        .map(|(name, p)| (name.as_str(), p.windows[0].used))
        .collect();
    assert_eq!(shown, [(POOL_CLAUDE, 0.25)]);
    assert_eq!(std::fs::read_to_string(&over).unwrap(), text);
    assert!(
        !horch_core::telemetry::dir(&w.state)
            .join("quota.json")
            .exists(),
        "the state's own file is not made"
    );

    // Without the override the tick writes the state's own file.
    let mut c = collector(&w);
    c.tick(now()).unwrap();
    assert!(horch_core::telemetry::dir(&w.state)
        .join("quota.json")
        .is_file());
}

#[test]
fn a_cursor_whose_input_file_is_gone_is_dropped_by_the_tidy() {
    const SID: &str = "55555555-5555-4555-8555-555555555555";
    let w = world(Part::Whole);
    let from = w
        .home
        .join(format!(".claude/projects/-work-alpha/{A2}.jsonl"));
    let path = w
        .home
        .join(format!(".claude/projects/-work-alpha/{SID}.jsonl"));
    std::fs::copy(from, &path).unwrap();
    let rows = serde_json::json!([{
        "record_id": "rec-gone", "session_id": SID, "agent": "claude", "tier": "sonnet",
        "model": "claude-sonnet-5", "role": "gone", "status": "done", "task": "t",
        "history": [], "created_at": "2026-09-01T00:00:00Z", "updated_at": "2026-09-28T17:00:00Z"
    }]);
    std::fs::write(w.state.join("-gone.json"), rows.to_string()).unwrap();
    let cursors = |w: &World| {
        std::fs::read_to_string(horch_core::telemetry::dir(&w.state).join("cursors.json")).unwrap()
    };
    let mut c = collector(&w);
    c.tick(now()).unwrap();
    assert!(cursors(&w).contains(SID));
    std::fs::remove_file(&path).unwrap();
    c.tick(now() + chrono::Duration::minutes(TIDY_EVERY_MIN))
        .unwrap();
    assert!(!cursors(&w).contains(SID), "{}", cursors(&w));
}

#[test]
fn live_rows_show_working_records_and_hide_finished_ones_with_no_recent_event() {
    let w = world(Part::Whole);
    ledger(
        &w,
        "-live.json",
        &[
            ("zeta", "worker", "working", "z task"),
            ("alpha", "worker", "working", "a task"),
            ("boss", "orchestrator", "working", "o task"),
            ("old", "worker", "done", "finished long ago"),
            (
                "waiting",
                "worker",
                "working",
                horch_core::telemetry::collect::IDLE_TASK,
            ),
        ],
    );
    let snap = collector(&w).tick(now()).unwrap();
    let rows: Vec<_> = snap
        .live
        .iter()
        .filter(|l| l.project.as_deref() == Some("/p"))
        .map(|l| (l.role.as_str(), l.idle, l.task_head.as_str()))
        .collect();
    // The orchestrator first, then by role. A working record with no recent
    // event is idle; a done record with none is not listed.
    assert_eq!(
        rows,
        vec![
            ("boss", true, "o task"),
            ("alpha", true, "a task"),
            ("waiting", true, horch_core::telemetry::collect::IDLE_TASK),
            ("zeta", true, "z task"),
        ]
    );
}
