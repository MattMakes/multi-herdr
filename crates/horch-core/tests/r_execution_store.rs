//! Ledger record conversion (`to_execution`), `ExecutionStore::read` and
//! `end_if_pane_closed`: each result and each guard.

use horch_core::execution::legacy::{
    LedgerRecordV1, KIND_ORCHESTRATOR, KIND_WORKER, STATUS_WORKING,
};
use horch_core::execution::store::{to_execution, ExecutionStore, LegacyError, PANE_CLOSED};
use horch_core::execution::{ExecutionKind, ExecutionStatus, FailureKind, SessionState};
use horch_core::workspace::client::WorkspaceClient;
use horch_core::workspace::testing::FakeWorkspace;

fn record() -> LedgerRecordV1 {
    LedgerRecordV1 {
        record_id: "rec-1".into(),
        session_id: Some("sess-1".into()),
        agent: "claude".into(),
        tier: "sonnet".into(),
        model: "sonnet".into(),
        role: "sonnet-1".into(),
        kind: KIND_WORKER.into(),
        status: "done".into(),
        task: "work".into(),
        created_at: "2026-09-28T17:00:00Z".into(),
        updated_at: "2026-09-28T17:30:00Z".into(),
        ..LedgerRecordV1::default()
    }
}

fn kind_error(r: &LedgerRecordV1) -> String {
    match to_execution(r).unwrap_err() {
        LegacyError::Kind { record, reason } => {
            assert_eq!(record, r.record_id);
            reason
        }
        other => panic!("{other}"),
    }
}

#[test]
fn kind_of_a_plain_worker_and_an_orchestrator() {
    assert_eq!(to_execution(&record()).unwrap().kind, ExecutionKind::Worker);
    let mut o = record();
    o.kind = KIND_ORCHESTRATOR.into();
    assert_eq!(to_execution(&o).unwrap().kind, ExecutionKind::Orchestrator);
}

#[test]
fn kind_of_an_orchestrator_with_a_label_is_refused() {
    let mut o = record();
    o.kind = KIND_ORCHESTRATOR.into();
    o.label = Some("A".into());
    assert_eq!(
        kind_error(&o),
        "an orchestrator has no experiment, round or label"
    );
}

#[test]
fn kind_of_an_unknown_kind_is_refused() {
    let mut r = record();
    r.kind = "wizard".into();
    assert_eq!(kind_error(&r), "unknown kind 'wizard'");
}

#[test]
fn kind_of_a_judge_and_a_candidate() {
    let mut j = record();
    j.round_id = Some("r1".into());
    j.label = Some("judge:2".into());
    assert_eq!(
        to_execution(&j).unwrap().kind,
        ExecutionKind::Judge {
            round: "r1".parse().unwrap(),
            attempt: 2
        }
    );
    let mut c = record();
    c.experiment_id = Some("e1".into());
    c.round_id = Some("r1".into());
    c.label = Some("A".into());
    assert_eq!(
        to_execution(&c).unwrap().kind,
        ExecutionKind::Candidate {
            experiment: "e1".parse().unwrap(),
            round: "r1".parse().unwrap(),
            label: "A".into()
        }
    );
}

#[test]
fn kind_of_a_judge_label_without_an_attempt_number_is_refused() {
    let mut j = record();
    j.round_id = Some("r1".into());
    j.label = Some("judge:x".into());
    assert_eq!(
        kind_error(&j),
        "judge label 'judge:x' has no attempt number"
    );
}

#[test]
fn kind_of_a_partial_kind_is_refused() {
    let mut r = record();
    r.round_id = Some("r1".into());
    assert_eq!(
        kind_error(&r),
        "experiment_id, round_id and label do not name a kind"
    );
}

#[test]
fn session_none_empty_and_known() {
    let mut r = record();
    r.session_id = None;
    assert_eq!(to_execution(&r).unwrap().session, SessionState::Pending);
    r.session_id = Some(String::new());
    assert_eq!(to_execution(&r).unwrap().session, SessionState::Unavailable);
    r.session_id = Some("sess-9".into());
    assert_eq!(
        to_execution(&r).unwrap().session,
        SessionState::Known("sess-9".parse().unwrap())
    );
}

#[test]
fn a_bad_id_or_agent_is_refused() {
    let mut r = record();
    r.agent = "nope".into();
    assert!(matches!(
        to_execution(&r).unwrap_err(),
        LegacyError::Agent(_)
    ));
    let mut r = record();
    r.record_id = String::new();
    assert!(matches!(to_execution(&r).unwrap_err(), LegacyError::Id(_)));
    let mut r = record();
    r.task_id = Some(String::new());
    assert!(matches!(to_execution(&r).unwrap_err(), LegacyError::Id(_)));
    let mut r = record();
    r.workspace_id = Some(String::new());
    assert!(matches!(to_execution(&r).unwrap_err(), LegacyError::Id(_)));
    let mut r = record();
    r.pane_id = Some(String::new());
    assert!(matches!(to_execution(&r).unwrap_err(), LegacyError::Id(_)));
}

fn store() -> (tempfile::TempDir, ExecutionStore) {
    let tmp = tempfile::tempdir().unwrap();
    let store = ExecutionStore::for_project(tmp.path(), "/work/alpha");
    (tmp, store)
}

#[test]
fn read_a_missing_empty_and_valid_ledger() {
    let (_tmp, store) = store();
    assert!(store.read().unwrap().is_empty(), "missing file");
    std::fs::write(store.path(), "  \n").unwrap();
    assert!(store.read().unwrap().is_empty(), "blank file");
    store
        .update(|rs| {
            rs.push(record());
            Ok(())
        })
        .unwrap();
    let records = store.read().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, "rec-1");
    std::fs::write(store.path(), "{not json").unwrap();
    let err = store.read().unwrap_err();
    assert!(format!("{err:#}").contains("parsing ledger"), "{err:#}");
}

/// A working worker record in pane `p9`, and a fake workspace without it.
fn working() -> LedgerRecordV1 {
    let mut r = record();
    r.status = STATUS_WORKING.into();
    r.pane_id = Some("p9".into());
    r
}

fn put(store: &ExecutionStore, r: LedgerRecordV1) {
    store
        .update(|rs| {
            rs.push(r);
            Ok(())
        })
        .unwrap();
}

#[test]
fn end_if_pane_closed_ends_a_working_record_whose_pane_is_gone() {
    let (_tmp, store) = store();
    put(&store, working());
    let ws = FakeWorkspace::new();
    assert!(store.end_if_pane_closed("rec-1", &ws).unwrap());
    let r = store.get("rec-1").unwrap();
    assert_eq!(
        r.execution_status(),
        ExecutionStatus::Failed {
            failure: FailureKind::PaneVanished
        }
    );
    assert!(r.finished_at.is_some());
    let last = r.history.last().unwrap();
    assert_eq!(
        (last.event.as_str(), last.text.as_str()),
        ("ended", PANE_CLOSED)
    );
    // A second look finds a finished record.
    assert!(!store.end_if_pane_closed("rec-1", &ws).unwrap());
}

#[test]
fn end_if_pane_closed_leaves_the_record_alone_under_each_guard() {
    let ws_open = FakeWorkspace::new();
    ws_open.workspace_create("w1", None, false).unwrap();
    // The pane `p1` exists in the workspace.
    let mut open_pane = working();
    open_pane.pane_id = Some("p1".into());
    let mut no_pane = working();
    no_pane.pane_id = None;
    let mut done = working();
    done.status = "done".into();
    let mut orch = working();
    orch.kind = KIND_ORCHESTRATOR.into();
    let cases: Vec<(&str, LedgerRecordV1, &FakeWorkspace)> = vec![
        ("pane still open", open_pane, &ws_open),
        ("no pane recorded", no_pane, &ws_open),
        ("not working", done, &ws_open),
        ("orchestrator", orch, &ws_open),
    ];
    for (name, r, ws) in cases {
        let (_tmp, store) = store();
        let before = r.clone();
        put(&store, r);
        assert!(!store.end_if_pane_closed("rec-1", ws).unwrap(), "{name}");
        let after = store.get("rec-1").unwrap();
        assert_eq!(after.status, before.status, "{name}");
        assert_eq!(after.finished_at, before.finished_at, "{name}");
        assert_eq!(after.history.len(), before.history.len(), "{name}");
    }
    // herdr does not answer: a gone pane proves nothing.
    let (_tmp, store) = store();
    put(&store, working());
    let down = FakeWorkspace::new();
    down.set_reachable(false);
    assert!(!store.end_if_pane_closed("rec-1", &down).unwrap());
    assert_eq!(store.get("rec-1").unwrap().status, STATUS_WORKING);
    // No record matches the key.
    assert!(!store.end_if_pane_closed("nope", &down).unwrap());
}
