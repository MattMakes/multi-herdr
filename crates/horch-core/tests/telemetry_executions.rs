//! Telemetry reads executions through the store (A6: ARC-23).
//!
//! Live rows come from the typed status: only a starting or running
//! execution is live, a failed launch never is, a ledger written before the
//! typed status still loads, and the dataset tree is never read as a ledger.

mod common;

use std::path::Path;

use horch_core::clock;
use horch_core::execution::legacy::LedgerRecordV1;
use horch_core::execution::store::{self, ExecutionStore};
use horch_core::execution::{ExecutionStatus, FailureKind, LaunchStage};
use horch_core::routing::policy::Policy;
use horch_core::routing::quota::{QuotaFile, QuotaView};
use horch_core::telemetry::collect::{
    build_snapshot, done_record_ids, read_ledgers, Collector, CollectorInfo, Probing,
};
use horch_core::telemetry::{Event, TokenClasses};

const NOW: &str = "2026-10-02T12:00:00Z";

fn now() -> chrono::DateTime<chrono::Utc> {
    clock::parse(NOW).unwrap()
}

fn record(id: &str, state: Option<ExecutionStatus>) -> LedgerRecordV1 {
    let mut r = LedgerRecordV1 {
        record_id: id.to_string(),
        session_id: None,
        agent: "claude".into(),
        tier: "sonnet".into(),
        model: "claude-sonnet-5-5".into(),
        role: id.to_string(),
        status: "working".into(),
        task: "ai_docs/plans/x.md".into(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
        project: Some("/work/typed".into()),
        ..LedgerRecordV1::default()
    };
    if let Some(s) = state {
        r.set_state(s);
    }
    r
}

/// One record in every typed state, keyed by the role it runs as.
fn typed_records() -> Vec<LedgerRecordV1> {
    vec![
        record("planned", Some(ExecutionStatus::Planned)),
        record("starting", Some(ExecutionStatus::Starting)),
        record("running", Some(ExecutionStatus::Running)),
        record("done", Some(ExecutionStatus::Done)),
        record(
            "failed",
            Some(ExecutionStatus::Failed {
                failure: FailureKind::AgentExited { code: Some(2) },
            }),
        ),
        record(
            "launch-failed",
            Some(ExecutionStatus::LaunchFailed {
                stage: LaunchStage::Split,
                reason: "no such pane".into(),
            }),
        ),
    ]
}

/// A ledger in the format before the typed status: no `state` key.
const OLD_LEDGER: &str = r#"[
  {
    "record_id": "old-working",
    "session_id": null,
    "agent": "codex",
    "tier": "codex-sol",
    "model": "gpt-5",
    "role": "codex-sol-1",
    "status": "working",
    "task": "old task",
    "history": [],
    "created_at": "2026-10-02T11:00:00Z",
    "updated_at": "2026-10-02T11:00:00Z"
  },
  {
    "record_id": "old-done",
    "session_id": null,
    "agent": "codex",
    "tier": "codex-sol",
    "model": "gpt-5",
    "role": "codex-sol-2",
    "status": "done",
    "task": "old task",
    "history": [],
    "created_at": "2026-10-02T11:00:00Z",
    "updated_at": "2026-10-02T11:00:00Z"
  }
]"#;

/// A state root with the typed ledger, the old ledger, and a valid-looking
/// ledger inside the dataset tree.
fn state_root(root: &Path) {
    let typed = ExecutionStore::for_project(root, "/work/typed");
    std::fs::write(
        typed.path(),
        ExecutionStore::render_json(&typed_records()).unwrap(),
    )
    .unwrap();
    let old = ExecutionStore::for_project(root, "/work/old");
    std::fs::write(old.path(), OLD_LEDGER).unwrap();
    let dataset = root.join("multi-herdr/-work-typed");
    std::fs::create_dir_all(&dataset).unwrap();
    std::fs::write(
        dataset.join("ledger.json"),
        ExecutionStore::render_json(&[record("dataset", Some(ExecutionStatus::Running))]).unwrap(),
    )
    .unwrap();
}

fn event(record_id: &str, ts: &str) -> Event {
    Event {
        ts: ts.into(),
        project: Some("/work/typed".into()),
        record_id: record_id.into(),
        session_id: format!("sid-{record_id}"),
        event_id: format!("ev-{record_id}"),
        role: record_id.into(),
        teammate: "sonnet".into(),
        via: None,
        kind: "worker".into(),
        agent: "claude".into(),
        model: "claude-sonnet-5-5".into(),
        effort: None,
        phase: None,
        plan: None,
        subagent: false,
        delta: false,
        tool_nested: false,
        idle: false,
        tokens: TokenClasses::default(),
        cost_usd: Some(0.01),
        harness_cost: None,
    }
}

fn live_ids(rows: &[horch_core::telemetry::collect::LiveRow]) -> Vec<String> {
    let mut ids: Vec<String> = rows.iter().map(|r| r.record_id.clone()).collect();
    ids.sort();
    ids
}

#[test]
fn arc_23_telemetry_reads_executions() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    std::fs::create_dir_all(&root).unwrap();
    state_root(&root);

    // The store reads both ledgers and skips the dataset tree.
    let ledgers = store::read_all_ledgers(&root);
    assert_eq!(ledgers.len(), 2, "{ledgers:?}");
    let all = read_ledgers(&root);
    let ids: Vec<&str> = all.iter().map(|r| r.record_id.as_str()).collect();
    assert!(!ids.contains(&"dataset"), "dataset tree read: {ids:?}");
    assert_eq!(all.len(), 8, "{ids:?}");

    // The old ledger loads; its legacy words resolve to typed states.
    let old_working = all.iter().find(|r| r.record_id == "old-working").unwrap();
    assert!(old_working.state.is_none());
    assert_eq!(old_working.execution_status(), ExecutionStatus::Running);

    // A collector tick: live rows only for Starting and Running.
    let home = tmp.path().join("home");
    let mut c = Collector::open_at(&root, common::locations(&home), Probing::Never, now()).unwrap();
    let snap = c.tick(now()).unwrap();
    assert_eq!(
        live_ids(&snap.live),
        vec!["old-working", "running", "starting"],
        "Planned, Done, Failed and LaunchFailed are not live"
    );
    // The legacy word stays in the row.
    assert!(snap.live.iter().all(|r| r.status == "working"));

    // Every terminal state counts as done; Planned does not.
    let done = done_record_ids(&all);
    let mut done: Vec<&str> = done.iter().map(String::as_str).collect();
    done.sort();
    assert_eq!(done, vec!["done", "failed", "launch-failed", "old-done"]);
}

/// A recent event shows a finished record as a live row, as before. A
/// failed one never shows: no agent runs for it.
#[test]
fn arc_23_failed_executions_are_never_live() {
    let records = typed_records();
    let recent = "2026-10-02T11:58:00Z";
    let events: Vec<Event> = ["done", "failed", "launch-failed", "planned"]
        .iter()
        .map(|id| event(id, recent))
        .collect();
    let policy = Policy::default();
    let quota = QuotaView::new(QuotaFile::default(), now(), policy.clone(), false);
    let snap = build_snapshot(
        &records,
        &events,
        quota,
        Vec::new(),
        now(),
        &policy,
        CollectorInfo::default(),
    );
    assert_eq!(
        live_ids(&snap.live),
        vec!["done", "planned", "running", "starting"]
    );
    let idle: Vec<(&str, bool)> = snap
        .live
        .iter()
        .map(|r| (r.record_id.as_str(), r.idle))
        .collect();
    // Starting and Running have no recent event, so they are idle.
    assert!(idle.contains(&("starting", true)), "{idle:?}");
    assert!(idle.contains(&("done", false)), "{idle:?}");
}
