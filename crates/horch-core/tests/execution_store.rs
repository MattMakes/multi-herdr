//! The execution store and the ledger facade over it (A6: ARC-17, SKL-04).
//!
//! Old ledgers load, resume and save byte for byte; an old binary reads a
//! typed state as the right legacy `status`; concurrent writers do not lose
//! records.

use std::path::{Path, PathBuf};
use std::time::Duration;

use horch_core::execution::legacy::{HistoryEntry, LedgerRecordV1};
use horch_core::execution::legacy::{Record, KIND_WORKER, STATUS_DONE, STATUS_WORKING};
use horch_core::execution::records::Ledger;
use horch_core::execution::store::ExecutionStore;
use horch_core::execution::{ExecutionStatus, FailureKind, LaunchStage};
use horch_core::ids::SkillId;
use horch_core::measure::digest::Digest;
use horch_core::roster::Phase;
use horch_core::skills::{InvocationPolicy, ResolvedSkillRef, SkillVersion};
use serde::Deserialize;

const LEDGERS: [&str; 4] = ["bash-era", "pre-effort", "pr14-substituted", "orchestrator"];

fn oracle_ledger(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/oracles/ledgers/{name}"))
}

/// A store at `<tmp>/<slug>.json` holding a copy of the A0 fixture `name`.
fn fixture_store(tmp: &Path, name: &str) -> ExecutionStore {
    let store = ExecutionStore::for_project(tmp, &format!("/oracle/{name}"));
    std::fs::copy(oracle_ledger(&format!("{name}.json")), store.path()).unwrap();
    store
}

fn ledger() -> (tempfile::TempDir, Ledger) {
    let tmp = tempfile::tempdir().unwrap();
    let l = Ledger::for_project(tmp.path(), "/Users/a/proj");
    (tmp, l)
}

// ─── ARC-17 ─────────────────────────────────────────────────────────────────

#[test]
fn arc_17_legacy_ledgers_load_and_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let mut resumed = 0;
    for name in LEDGERS {
        let store = fixture_store(tmp.path(), name);
        let records = store.read().unwrap();
        assert!(!records.is_empty(), "{name} loads no records");
        for r in &records {
            assert_eq!(r.state, None, "{name}: a legacy record has no state");
            assert_eq!(
                r.execution_status(),
                ExecutionStatus::from_legacy(&r.status),
                "{name}"
            );
        }
        let Some(worker) = records.iter().find(|r| r.kind == KIND_WORKER) else {
            continue;
        };
        let ledger = Ledger::for_project(tmp.path(), &format!("/oracle/{name}"));
        ledger
            .resume(&worker.record_id, "opus-9", "resume ai_docs/plans/next.md")
            .unwrap();
        let after = store.read().unwrap();
        assert_eq!(after.len(), records.len(), "{name}");
        let r = store.get(&worker.record_id).unwrap();
        assert_eq!(r.status, STATUS_WORKING, "{name}");
        assert_eq!(r.role, "opus-9", "{name}");
        assert_eq!(r.plan.as_deref(), Some("next"), "{name}");
        assert_eq!(r.history.last().unwrap().event, "resumed", "{name}");
        assert_eq!(r.state, None, "{name}: resume adds no typed state");
        assert!(r.execution_status().is_live(), "{name}");
        resumed += 1;
    }
    assert!(resumed >= 3, "only {resumed} fixtures had a worker record");
}

/// The store writes `to_string_pretty` with no trailing newline, the same
/// bytes as the A0 `.saved.json` oracle, so no normalization is needed.
#[test]
fn arc_17_roundtrip_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    for name in LEDGERS {
        let store = fixture_store(tmp.path(), name);
        store.update(|_| Ok(())).unwrap();
        let saved = std::fs::read(store.path()).unwrap();
        let want = std::fs::read(oracle_ledger(&format!("{name}.saved.json"))).unwrap();
        assert!(
            saved == want,
            "{name}: a load and save changed the bytes:\n{}",
            String::from_utf8_lossy(&saved)
        );
        // A second pass is stable too.
        store.update(|_| Ok(())).unwrap();
        assert_eq!(std::fs::read(store.path()).unwrap(), want, "{name}");
    }
}

/// The pre-A6 `ledger::Record`, copied as it was, to read new ledgers the
/// way an old binary does.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OldRecord {
    record_id: String,
    session_id: Option<String>,
    agent: String,
    tier: String,
    model: String,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    phase: Option<Phase>,
    role: String,
    status: String,
    task: String,
    history: Vec<HistoryEntry>,
    created_at: String,
    updated_at: String,
    #[serde(default = "old_worker_kind")]
    kind: String,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    plan: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    via: Option<String>,
    #[serde(default)]
    substitution_reason: Option<String>,
    #[serde(default)]
    routing: Option<serde_json::Value>,
}

fn old_worker_kind() -> String {
    "worker".to_string()
}

fn every_status() -> Vec<ExecutionStatus> {
    let mut all = vec![
        ExecutionStatus::Planned,
        ExecutionStatus::Starting,
        ExecutionStatus::Running,
        ExecutionStatus::Done,
    ];
    for failure in [
        FailureKind::AgentExited { code: Some(1) },
        FailureKind::AgentExited { code: None },
        FailureKind::PaneVanished,
        FailureKind::TimedOut,
        FailureKind::Cancelled {
            reason: "operator".into(),
        },
        FailureKind::Crashed,
    ] {
        all.push(ExecutionStatus::Failed { failure });
    }
    for stage in [LaunchStage::Brief, LaunchStage::Split, LaunchStage::Run] {
        all.push(ExecutionStatus::LaunchFailed {
            stage,
            reason: "no such pane".into(),
        });
    }
    all
}

#[test]
fn arc_17_old_reader_sees_compat_status() {
    let (_t, l) = ledger();
    let statuses = every_status();
    for (i, state) in statuses.iter().enumerate() {
        let id = format!("r{i}");
        l.add(&id, "claude", "sonnet", "sonnet", "sonnet-1", None, "t")
            .unwrap();
        l.store().set_state(&id, state.clone()).unwrap();
    }

    let raw = std::fs::read_to_string(l.path()).unwrap();
    let old: Vec<OldRecord> = serde_json::from_str(&raw).unwrap();
    assert_eq!(old.len(), statuses.len());
    for (r, state) in old.iter().zip(&statuses) {
        let want = match state {
            ExecutionStatus::Planned | ExecutionStatus::Starting | ExecutionStatus::Running => {
                STATUS_WORKING
            }
            _ => STATUS_DONE,
        };
        assert_eq!(r.status, want, "{state:?}");
    }

    // A new reader gets the typed state back exactly.
    let new = l.read().unwrap();
    for (r, state) in new.iter().zip(&statuses) {
        assert_eq!(r.execution_status(), *state);
        assert_eq!(r.finished_at.is_some(), state.is_terminal(), "{state:?}");
    }
    let live: Vec<String> = l
        .store()
        .live()
        .unwrap()
        .into_iter()
        .map(|r| r.record_id)
        .collect();
    assert_eq!(live, ["r1", "r2"], "only Starting and Running are live");

    // The documented shape: a nested object under `state`.
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value[11]["state"],
        serde_json::json!({"state": "launch_failed", "stage": "split", "reason": "no such pane"})
    );
    assert_eq!(value[11]["status"], "done");

    // The legacy setters keep `status` and `state` in step.
    l.resume("r11", "sonnet-2", "").unwrap();
    assert_eq!(
        l.get("r11").unwrap().execution_status(),
        ExecutionStatus::Running
    );
    l.done("r0", "finished").unwrap();
    let r0 = l.get("r0").unwrap();
    assert_eq!(
        (r0.status.as_str(), r0.execution_status()),
        (STATUS_DONE, ExecutionStatus::Done)
    );
}

// ─── SKL-04 ─────────────────────────────────────────────────────────────────

#[test]
fn skl_04_execution_records_skill_refs() {
    let (_t, l) = ledger();
    l.add("r1", "claude", "opus", "opus", "opus-1", Some("s1"), "t")
        .unwrap();
    let raw = std::fs::read_to_string(l.path()).unwrap();
    assert!(
        !raw.contains("\"skills\""),
        "no skills stay off disk: {raw}"
    );

    let skills = vec![
        ResolvedSkillRef {
            id: SkillId::new("tdd").unwrap(),
            version: SkillVersion("bundled+0123456789ab".into()),
            digest: Digest([7; 32]),
            source: "bundled".into(),
            policy: InvocationPolicy::Explicit,
        },
        ResolvedSkillRef {
            id: SkillId::new("debug").unwrap(),
            version: SkillVersion("git+abababababab".into()),
            digest: Digest([9; 32]),
            source: "https://example.com/r.git@abababababab".into(),
            policy: InvocationPolicy::Deterministic,
        },
    ];
    l.store().set_skills("s1", skills.clone()).unwrap();
    assert_eq!(l.get("r1").unwrap().skills, skills);

    // The skills survive an unrelated write through the facade.
    l.note("r1", "halfway").unwrap();
    assert_eq!(l.get("r1").unwrap().skills, skills);
    assert!(l.store().set_skills("missing", Vec::new()).is_err());
}

// ─── locking ────────────────────────────────────────────────────────────────

#[test]
fn store_dirlock_excludes_concurrent_writers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let writers: Vec<_> = (0..2)
        .map(|w| {
            let root = root.clone();
            std::thread::spawn(move || {
                let store = ExecutionStore::for_project(&root, "/p");
                for n in 0..50 {
                    store
                        .insert(LedgerRecordV1 {
                            record_id: format!("w{w}-{n}"),
                            ..LedgerRecordV1::default()
                        })
                        .unwrap();
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    let records = ExecutionStore::for_project(&root, "/p").read().unwrap();
    assert_eq!(records.len(), 100);
    for w in 0..2 {
        for n in 0..50 {
            let id = format!("w{w}-{n}");
            assert!(records.iter().any(|r| r.record_id == id), "lost {id}");
        }
    }
    assert!(!root.join("-p.json.lock").exists(), "the lock is released");
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
