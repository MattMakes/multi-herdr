//! B1 measurement layer: dataset paths, the event log, the projection fold
//! and WorkerRun 1.0.0 (dataset design §2 and §4.1 to §4.3).

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use horch_core::competition::model::RoundState;
use horch_core::competition::preflight::PreflightReport;
use horch_core::evaluation::validator::ValidationReport;
use horch_core::evaluation::winner::RejectReason;
use horch_core::execution::FailureKind;
use horch_core::harness::HarnessKind;
use horch_core::ids::{
    EventId, ExecutionId, ExperimentId, JudgmentId, ModelId, PaneId, RoundId, TaskId, TeammateName,
};
use horch_core::measure::digest::{sha256_bytes, Digest};
use horch_core::measure::event::*;
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::{fold, Projection};
use horch_core::measure::recorder::{Appended, JsonlRecorder, NewEvent, NoopRecorder, Recorder};
use horch_core::measure::store::{EventIndex, StoreOptions};
use horch_core::measure::testkit::{property, SplitMix64};
use horch_core::measure::NumstatLine;
use horch_core::routing::decision::RoutingProvenance;
use horch_core::teacher::TeacherRef;
use horch_core::telemetry::collect::read_ledgers;
use serde_json::{json, Value};

// ─── fixtures ───────────────────────────────────────────────────────────────

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 2, 12, 0, 0).unwrap()
}

fn dg(tag: &str) -> Digest {
    sha256_bytes(tag.as_bytes())
}

fn exp_id() -> ExperimentId {
    ExperimentId::new("0199a5b0-0000-7000-8000-000000000001").unwrap()
}

fn round_id() -> RoundId {
    RoundId::new("0199a5b0-0000-7000-8000-000000000002").unwrap()
}

fn exec_id(label: &str) -> ExecutionId {
    let n = label.bytes().next().unwrap_or(b'A');
    ExecutionId::new(format!("0199a5b0-0000-7000-8000-0000000000{n:02x}")).unwrap()
}

fn experiment_created(labels: u32) -> ExperimentCreated {
    ExperimentCreated {
        task_id: TaskId::new("task-1").unwrap(),
        task_digest: dg("task"),
        config_digest: dg("config"),
        base_sha: "a".repeat(40),
        repo_digest: dg("repo"),
        environment_digest: dg("env"),
        candidates: labels,
        strategy: "baseline+diversity".into(),
        budget_usd_micro: 5_000_000,
        promote_to: None,
    }
}

fn round_created(labels: &[&str]) -> RoundCreated {
    RoundCreated {
        index: 0,
        base_sha: "a".repeat(40),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        eligible_set: typed(json!([
            {"teammate": "sonnet", "harness": "claude", "model": "sonnet", "effort": "high",
             "fallback_index": null, "pool": "claude-max", "pool_state": "ok",
             "verdict": "eligible"},
            {"teammate": "codex-sol", "harness": "codex", "model": "gpt-5-codex", "effort": null,
             "fallback_index": null, "pool": "codex", "pool_state": "exhausted",
             "verdict": "excluded", "reason": "pool_blocked"},
        ])),
        propensities: labels
            .iter()
            .map(|l| (l.to_string(), 1.0 / labels.len() as f64))
            .collect(),
        teacher: TeacherRef::none(),
        seed: 42,
        label_policy_version: "lp-1".into(),
    }
}

fn candidate_planned(label: &str) -> CandidatePlanned {
    CandidatePlanned {
        label: label.into(),
        teammate: TeammateName::new("sonnet").unwrap(),
        harness: HarnessKind::Claude,
        model: ModelId::new("sonnet").unwrap(),
        effort: Some("high".into()),
        slot: SlotKind::Baseline,
        propensity: 0.5,
        config_id: "sonnet|claude|sonnet|high".into(),
    }
}

fn typed<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}

fn validation_report(label: &str, eligible: bool) -> ValidationReport {
    let status = if eligible {
        json!({"kind": "passed"})
    } else {
        json!({"kind": "failed", "code": 101})
    };
    typed(json!({
        "validation_id": "0199a5b0-0000-7000-8000-0000000000f1",
        "label": label,
        "head_sha": "b".repeat(40),
        "gates": [{"name": "test", "status": status, "duration_ms": 1200,
                   "log_ref": format!("validation/{label}/test.log"),
                   "log_digest": dg("log").to_string(), "log_truncated": false}],
        "mechanical_score": if eligible { 1.0 } else { 0.0 },
        "eligible": eligible,
    }))
}

fn preflight_report() -> PreflightReport {
    typed(json!({
        "schema_version": "1.0.0",
        "checks": [{"id": "PRE-01", "status": "pass", "detail": "git ok", "measured": null}],
        "safe_n": 2,
        "waves": 1,
        "projected_cost_microusd": 1_500_000,
        "machine": {"os": "macos", "arch": "aarch64", "cpus": 10,
                    "mem_total_bytes": 34_359_738_368u64, "mem_available_bytes": null,
                    "disk_free_bytes": 100_000_000_000u64, "disk_total_bytes": null,
                    "gpu": "apple_silicon", "max_open_files": 10240, "max_processes": null},
        "environment_digest": dg("env").to_string(),
        "passed": true,
    }))
}

fn routing(teammate: &str) -> RoutingProvenance {
    typed(json!({
        "requested": teammate, "resolved": teammate, "fallback_index": null,
        "pool": "claude-max", "pool_state": "ok", "reason": null, "mode": "pinned",
    }))
}

fn numstat() -> Vec<NumstatLine> {
    typed(json!([
        {"added": 3, "deleted": 1, "path": "src/lib.rs"},
        {"added": null, "deleted": null, "path": "assets/logo.png"},
    ]))
}

/// One sample of every known kind, in `EventKind::KNOWN` order.
fn sample_kinds() -> Vec<EventKind> {
    vec![
        EventKind::ExperimentCreated(experiment_created(2)),
        EventKind::PreflightCompleted(PreflightCompleted {
            report: preflight_report(),
        }),
        EventKind::ExperimentAborted(ExperimentAborted {
            reason: "disk".into(),
            failed_checks: vec!["PRE-02".into()],
        }),
        EventKind::RoundCreated(round_created(&["A", "B"])),
        EventKind::CandidatePlanned(candidate_planned("A")),
        EventKind::WorktreeCreated(WorktreeCreated {
            label: "A".into(),
            path: "/wt/A".into(),
            branch: "mh/exp/0199a5b0/r0/A".into(),
            base_sha: "a".repeat(40),
        }),
        EventKind::CandidateSpawned(CandidateSpawned {
            label: "A".into(),
            pane: PaneId::new("p-1").unwrap(),
            routing: routing("sonnet"),
        }),
        EventKind::CandidateCompleted(CandidateCompleted {
            label: "A".into(),
            exit_code: Some(0),
        }),
        EventKind::CandidateFailed(CandidateFailed {
            label: "B".into(),
            failure: FailureKind::AgentExited { code: Some(2) },
        }),
        EventKind::CandidateFrozen(CandidateFrozen {
            label: "A".into(),
            head_sha: "b".repeat(40),
            numstat: numstat(),
            diff_digest: dg("diff"),
        }),
        EventKind::ValidationCompleted(ValidationCompleted {
            label: "A".into(),
            report: validation_report("A", true),
        }),
        EventKind::JudgeScheduled(JudgeScheduled {
            attempt: 1,
            input_digest: dg("input"),
            judge_policy_digest: dg("policy"),
            job_dir: "/jobs/r/judge-1".into(),
        }),
        EventKind::JudgeStarted(JudgeStarted {
            attempt: 1,
            pid: 4242,
        }),
        EventKind::JudgeCompleted(JudgeCompleted {
            attempt: 1,
            judgment_id: JudgmentId::new("j-1").unwrap(),
            output_digest: dg("out"),
        }),
        EventKind::JudgeFailed(JudgeFailed {
            attempt: 1,
            cause: JudgeFailure::Crashed { code: Some(137) },
        }),
        EventKind::WinnerSelected(WinnerSelected {
            label: "A".into(),
            execution_id: exec_id("A"),
            head_sha: "b".repeat(40),
            judgment_id: JudgmentId::new("j-1").unwrap(),
            promotion: PromotionIntent::Requested {
                target: "main".into(),
            },
        }),
        EventKind::WinnerRejected(WinnerRejected {
            reason: RejectReason::NoEligible,
        }),
        EventKind::PromotionStarted(PromotionStarted {
            target: "main".into(),
            dest_before: "a".repeat(40),
            planned_after: "b".repeat(40),
            strategy: horch_core::competition::promotion::PromotionStrategy::FastForward,
            publish: "update_ref_cas".into(),
            validation_ids: vec!["v-1".into()],
        }),
        EventKind::PromotionCompleted(PromotionCompleted {
            receipt_digest: dg("receipt"),
            dest_after: "b".repeat(40),
        }),
        EventKind::PromotionConflicted(PromotionConflicted {
            paths: vec!["src/lib.rs".into()],
        }),
        EventKind::PromotionRolledBack(PromotionRolledBack {
            target: "main".into(),
            restored: "a".repeat(40),
        }),
        EventKind::WorktreeCleanupFailed(WorktreeCleanupFailed {
            label: "A".into(),
            path: "/wt/A".into(),
            error: "busy".into(),
        }),
        EventKind::OutcomeRecorded(OutcomeRecorded {
            kind: OutcomeKind::Verified,
            post_merge_score: 0.75,
            note: None,
        }),
        EventKind::RoundNeedsIntervention(RoundNeedsIntervention {
            reason: "tie".into(),
            source: InterventionSource::Judge,
        }),
        EventKind::RoundCleanupStarted(RoundCleanupStarted {}),
        EventKind::RoundCompleted(RoundCompleted {
            final_outcome: FinalOutcome::Winner,
        }),
        EventKind::OperatorPromote(OperatorPromote {
            target: "main".into(),
        }),
    ]
}

fn envelope(kind: &EventKind, at: DateTime<Utc>, key: &str) -> EventEnvelope {
    EventEnvelope {
        schema_version: EVENT_SCHEMA_VERSION.into(),
        event_id: EventId::mint(at),
        kind: kind.name().into(),
        occurred_at: format_occurred_at(at),
        actor: Actor::Coordinator,
        experiment_id: exp_id(),
        round_id: Some(round_id()),
        execution_id: Some(exec_id("A")),
        idempotency_key: key.into(),
        payload: kind.payload(),
    }
}

fn ledger_json(record_id: &str) -> String {
    json!([{
        "record_id": record_id, "session_id": "s-1", "agent": "claude", "tier": "sonnet",
        "model": "sonnet", "role": "sonnet-1", "status": "done", "task": "t",
        "history": [], "created_at": "2026-10-02T12:00:00Z",
        "updated_at": "2026-10-02T12:00:00Z"
    }])
    .to_string()
}

#[test]
fn mea_08_dataset_dir_not_read_as_ledger() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path();
    std::fs::write(state.join("proj.json"), ledger_json("rec-root")).unwrap();

    let paths = DatasetPaths::new(state, Path::new("/work/my proj"));
    paths.ensure().unwrap();
    assert_eq!(paths.root(), state.join("multi-herdr/-work-my-proj"));
    let exp = ExperimentId::new("exp-1").unwrap();
    std::fs::create_dir_all(paths.experiment_dir(&exp).unwrap()).unwrap();
    // Valid-looking ledgers at every dataset level, plus an event file.
    for p in [
        paths.root().join("ledger.json"),
        paths.root().parent().unwrap().join("ledger.json"),
        paths.manifest(&exp).unwrap(),
        paths.events_dir().join("x.json"),
    ] {
        std::fs::write(&p, ledger_json("rec-dataset")).unwrap();
    }
    std::fs::write(
        paths.events_file(chrono::NaiveDate::from_ymd_opt(2026, 10, 2).unwrap()),
        "{}\n",
    )
    .unwrap();

    let ids: Vec<String> = read_ledgers(state)
        .into_iter()
        .map(|r| r.record_id)
        .collect();
    assert_eq!(ids, vec!["rec-root".to_string()]);
}

#[test]
fn dataset_paths_layout() {
    let paths = DatasetPaths::from_slug(Path::new("/s"), "p");
    let exp = ExperimentId::new("e").unwrap();
    let round = horch_core::ids::RoundId::new("r").unwrap();
    let day = chrono::NaiveDate::from_ymd_opt(2026, 1, 9).unwrap();
    let want: Vec<(PathBuf, &str)> = vec![
        (
            paths.events_file(day),
            "/s/multi-herdr/p/events/2026-01-09.jsonl",
        ),
        (paths.events_lock_dir(), "/s/multi-herdr/p/events.lock"),
        (
            paths.manifest(&exp).unwrap(),
            "/s/multi-herdr/p/experiments/e/manifest.json",
        ),
        (
            paths.judge_input_dir(&exp, &round).unwrap(),
            "/s/multi-herdr/p/experiments/e/artifacts/r/judge-input",
        ),
        (
            paths.validation_dir(&exp, &round, "A").unwrap(),
            "/s/multi-herdr/p/experiments/e/artifacts/r/validation/A",
        ),
        (
            paths.judgement(&round).unwrap(),
            "/s/multi-herdr/p/judgements/r.json",
        ),
        (
            paths.promotion(&round).unwrap(),
            "/s/multi-herdr/p/promotions/r.json",
        ),
        (
            paths.exports_dir("lp-1").unwrap(),
            "/s/multi-herdr/p/exports/lp-1",
        ),
        (
            paths.job_dir(&round, 2).unwrap(),
            "/s/multi-herdr/p/jobs/r/judge-2",
        ),
        (
            paths.default_worktree_root(&exp).unwrap(),
            "/s/multi-herdr/p/worktrees/e",
        ),
    ];
    for (got, want) in want {
        assert_eq!(got, PathBuf::from(want));
    }
}

fn new_event(kind: EventKind, key: &str, at: DateTime<Utc>) -> NewEvent {
    NewEvent {
        kind,
        actor: Actor::Coordinator,
        experiment_id: exp_id(),
        round_id: Some(round_id()),
        execution_id: None,
        idempotency_key: key.into(),
        occurred_at: at,
    }
}

fn judge_started(attempt: u32, pid: u32) -> EventKind {
    EventKind::JudgeStarted(JudgeStarted { attempt, pid })
}

fn open_dataset(state: &Path) -> (DatasetPaths, JsonlRecorder) {
    let paths = DatasetPaths::from_slug(state, "proj");
    let rec = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();
    (paths, rec)
}

/// `(key, attempt, pid)` of every event, in read order.
fn keys_of(events: &[EventEnvelope]) -> Vec<(String, u32, u32)> {
    events
        .iter()
        .map(|e| match e.event().unwrap() {
            EventKind::JudgeStarted(j) => (e.idempotency_key.clone(), j.attempt, j.pid),
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

// ─── MEA-01, MEA-02: ids and the envelope ───────────────────────────────────

#[test]
fn mea_01_ids_v7() {
    let mut ids = Vec::new();
    for i in 0..64 {
        let at = t0() + chrono::Duration::milliseconds(i * 7);
        let id = EventId::mint(at);
        let uuid = uuid::Uuid::parse_str(id.as_str()).unwrap();
        assert_eq!(uuid.get_version_num(), 7, "{id}");
        assert_eq!(uuid.get_variant(), uuid::Variant::RFC4122);
        let ms = u64::from_be_bytes({
            let mut b = [0u8; 8];
            b[2..].copy_from_slice(&uuid.as_bytes()[..6]);
            b
        });
        assert_eq!(ms as i64, at.timestamp_millis());
        ids.push(id);
    }
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(sorted, ids, "v7 ids sort by time");
}

#[test]
fn mea_02_envelope_roundtrip_every_kind() {
    let samples = sample_kinds();
    let names: Vec<&str> = samples.iter().map(|k| k.name()).collect();
    assert_eq!(names, EventKind::KNOWN, "one sample per known kind");
    for (i, kind) in samples.iter().enumerate() {
        let at = t0() + chrono::Duration::milliseconds(i as i64);
        let env = envelope(kind, at, &format!("k:{i}"));
        let line = serde_json::to_string(&env).unwrap();
        let back: EventEnvelope = serde_json::from_str(&line).unwrap();
        assert_eq!(back, env, "{line}");
        assert_eq!(&back.event().unwrap(), kind);
        assert!(!matches!(back.event().unwrap(), EventKind::Unknown { .. }));
    }
}

#[test]
fn mea_02_occurred_at_is_utc_millis() {
    let at = Utc.with_ymd_and_hms(2026, 10, 2, 12, 0, 0).unwrap();
    assert_eq!(format_occurred_at(at), "2026-10-02T12:00:00.000Z");
    let at = at + chrono::Duration::microseconds(123_456);
    assert_eq!(format_occurred_at(at), "2026-10-02T12:00:00.123Z");
}

#[test]
fn mea_02_unknown_kind_preserved() {
    // Keys already in sorted order, so the bytes survive serde_json's map.
    let line = r#"{"schema_version":"1.0.0","event_id":"0199a5b0-0000-7000-8000-0000000000aa","kind":"candidate.teleported","occurred_at":"2026-10-02T12:00:00.000Z","actor":"worker","experiment_id":"e","idempotency_key":"tp:1","payload":{"a":[1,2.5,"x"],"nested":{"z":null}}}"#;
    let env: EventEnvelope = serde_json::from_str(line).unwrap();
    let kind = env.event().unwrap();
    assert_eq!(
        kind,
        EventKind::Unknown {
            kind: "candidate.teleported".into(),
            payload: json!({"a": [1, 2.5, "x"], "nested": {"z": null}}),
        }
    );
    assert_eq!(kind.name(), "candidate.teleported");
    assert_eq!(kind.payload(), env.payload);
    assert_eq!(serde_json::to_string(&env).unwrap(), line);
}

#[test]
fn mea_02_known_kind_bad_payload_is_an_error() {
    let err = EventKind::from_parts("judge.started", &json!({"attempt": "one"})).unwrap_err();
    assert!(matches!(err, EventError::BadPayload { .. }), "{err}");
}

// ─── MEA-03, MEA-04: the store and the recorders ────────────────────────────

#[test]
fn mea_03_torn_line_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, rec) = open_dataset(tmp.path());
    for i in 0..2 {
        rec.append(new_event(judge_started(i, 1), &format!("k{i}"), t0()))
            .unwrap();
    }
    let file = paths.events_file(t0().date_naive());
    let mut bytes = std::fs::read(&file).unwrap();
    bytes.extend_from_slice(b"not json at all\n");
    // A writer that crashed mid-line.
    bytes.extend_from_slice(br#"{"schema_version":"1.0.0","event_id":"x"#);
    std::fs::write(&file, &bytes).unwrap();

    let read = rec.read_all().unwrap();
    assert_eq!(read.events.len(), 2);
    assert_eq!(read.torn_lines, 2);

    // A new writer rebuilds its index past the torn tail, and its append
    // lands on a line of its own.
    let (_, rec2) = open_dataset(tmp.path());
    let out = rec2
        .append(new_event(judge_started(2, 1), "k2", t0()))
        .unwrap();
    assert!(matches!(out, Appended::Recorded(_)));
    let read = rec2.read_all().unwrap();
    assert_eq!(
        keys_of(&read.events),
        vec![
            ("k0".into(), 0, 1),
            ("k1".into(), 1, 1),
            ("k2".into(), 2, 1)
        ]
    );
    assert_eq!(read.torn_lines, 2);
    assert!(std::fs::read(&file).unwrap().ends_with(b"\n"));
}

#[test]
fn mea_03_two_process_appends_ordered() {
    const PER_WRITER: u32 = 200;
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().to_path_buf();
    // Each writer has its own recorder and index, as a separate process does.
    let writers: Vec<_> = (1..=2u32)
        .map(|w| {
            let state = state.clone();
            std::thread::spawn(move || {
                let (_, rec) = open_dataset(&state);
                for i in 0..PER_WRITER {
                    let at = t0() + chrono::Duration::milliseconds(i64::from(i));
                    let out = rec
                        .append(new_event(judge_started(i, w), &format!("w{w}:{i}"), at))
                        .unwrap();
                    assert!(matches!(out, Appended::Recorded(_)));
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    let (_, rec) = open_dataset(&state);
    let read = rec.read_all().unwrap();
    assert_eq!(read.torn_lines, 0);
    assert_eq!(read.events.len(), 2 * PER_WRITER as usize);
    for w in 1..=2u32 {
        let attempts: Vec<u32> = keys_of(&read.events)
            .into_iter()
            .filter(|(_, _, pid)| *pid == w)
            .map(|(_, attempt, _)| attempt)
            .collect();
        assert_eq!(attempts, (0..PER_WRITER).collect::<Vec<_>>(), "writer {w}");
    }
}

#[test]
fn mea_04_duplicate_key_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, rec) = open_dataset(tmp.path());
    let first = match rec
        .append(new_event(judge_started(1, 10), "spawn:r:A", t0()))
        .unwrap()
    {
        Appended::Recorded(env) => env,
        other => panic!("{other:?}"),
    };
    let file = paths.events_file(t0().date_naive());
    let before = std::fs::read(&file).unwrap();

    let again = rec
        .append(new_event(judge_started(9, 99), "spawn:r:A", t0()))
        .unwrap();
    assert_eq!(again, Appended::Duplicate(first.clone()));
    // A fresh recorder rebuilds the index from disk and agrees.
    let (_, rec2) = open_dataset(tmp.path());
    let again = rec2
        .append(new_event(judge_started(9, 99), "spawn:r:A", t0()))
        .unwrap();
    assert_eq!(again, Appended::Duplicate(first));
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

#[test]
fn mea_04_prop_duplicates_no_effect() {
    // Every case runs the recorder's dedup index; every 500th also runs
    // through a real JsonlRecorder. An append costs 2 full fsyncs on macOS,
    // so all 10 000 cases on disk would take minutes.
    let tmp = tempfile::tempdir().unwrap();
    let mut case = 0u32;
    property(0x6d65_615f_3034, 10_000, |rng| {
        case += 1;
        let len = 1 + rng.below(8) as u32;
        let keys = 1 + rng.below(4);
        let seq: Vec<(String, u32)> = (0..len)
            .map(|i| (format!("k{}", rng.below(keys)), i))
            .collect();
        let mut want: Vec<(String, u32)> = Vec::new();
        for (key, i) in &seq {
            if !want.iter().any(|(k, _)| k == key) {
                want.push((key.clone(), *i));
            }
        }

        let mut index = EventIndex::default();
        let mut log: Vec<(String, u32)> = Vec::new();
        for (key, i) in &seq {
            let env = envelope(&judge_started(*i, 0), t0(), key);
            match index.get(key) {
                Some(earlier) => assert_eq!(earlier.idempotency_key, *key),
                None => {
                    index.insert(env);
                    log.push((key.clone(), *i));
                }
            }
        }
        assert_eq!(log, want);
        assert_eq!(index.len(), want.len());

        if case % 500 != 0 {
            return;
        }
        let (_, rec) = open_dataset(&tmp.path().join(case.to_string()));
        for (key, i) in &seq {
            let out = rec
                .append(new_event(judge_started(*i, 0), key, t0()))
                .unwrap();
            let first = want.iter().find(|(k, _)| k == key).unwrap().1;
            assert_eq!(matches!(out, Appended::Duplicate(_)), first != *i);
        }
        let got: Vec<(String, u32)> = keys_of(&rec.read_all().unwrap().events)
            .into_iter()
            .map(|(k, attempt, _)| (k, attempt))
            .collect();
        assert_eq!(got, want);
    });
}

#[test]
fn store_options_from_faults() {
    use horch_core::runtime::fault::Faults;
    let on = StoreOptions::from_faults(&Faults::parse(Some("x,abort-after-event-append")));
    assert!(on.abort_after_append);
    assert_eq!(
        StoreOptions::from_faults(&Faults::parse(None)),
        StoreOptions::default()
    );
}

#[test]
fn noop_recorder_writes_nothing() {
    let out = NoopRecorder
        .append(new_event(judge_started(1, 1), "k", t0()))
        .unwrap();
    match out {
        Appended::Recorded(env) => {
            assert_eq!(env.kind, "judge.started");
            assert_eq!(env.occurred_at, "2026-10-02T12:00:00.000Z");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn empty_idempotency_key_is_refused() {
    assert!(NoopRecorder
        .append(new_event(judge_started(1, 1), "", t0()))
        .is_err());
}

#[cfg(unix)]
#[test]
fn measure_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let (paths, rec) = open_dataset(tmp.path());
    rec.append(new_event(judge_started(1, 1), "k", t0()))
        .unwrap();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    for dir in [
        paths.root().parent().unwrap().to_path_buf(),
        paths.root().to_path_buf(),
        paths.events_dir(),
        paths.experiments_dir(),
        paths.judgements_dir(),
        paths.promotions_dir(),
        paths.exports_root(),
        paths.jobs_root(),
        paths.worktrees_root(),
    ] {
        assert_eq!(mode(&dir), 0o700, "{}", dir.display());
    }
    assert_eq!(mode(&paths.events_file(t0().date_naive())), 0o600);
}

// ─── MEA-05: the projection fold ────────────────────────────────────────────

/// Builds a scripted event list with deterministic ids and times.
struct Script {
    events: Vec<EventEnvelope>,
}

impl Script {
    fn new() -> Self {
        Script { events: Vec::new() }
    }

    fn at(&self) -> DateTime<Utc> {
        t0() + chrono::Duration::milliseconds(self.events.len() as i64 * 250)
    }

    fn push(&mut self, kind: EventKind, label: Option<&str>) {
        let n = self.events.len();
        let experiment_level = matches!(
            kind,
            EventKind::ExperimentCreated(_)
                | EventKind::PreflightCompleted(_)
                | EventKind::ExperimentAborted(_)
        );
        self.events.push(EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION.into(),
            event_id: EventId::new(format!("evt-{n:05}")).unwrap(),
            kind: kind.name().into(),
            occurred_at: format_occurred_at(self.at()),
            actor: Actor::Coordinator,
            experiment_id: exp_id(),
            round_id: (!experiment_level).then(round_id),
            execution_id: label.map(exec_id),
            idempotency_key: format!("{}:{}:{n}", kind.name(), label.unwrap_or("-")),
            payload: kind.payload(),
        });
    }

    fn new_events(&self) -> Vec<NewEvent> {
        self.events
            .iter()
            .map(|e| NewEvent {
                kind: e.event().unwrap(),
                actor: e.actor,
                experiment_id: e.experiment_id.clone(),
                round_id: e.round_id.clone(),
                execution_id: e.execution_id.clone(),
                idempotency_key: e.idempotency_key.clone(),
                occurred_at: DateTime::parse_from_rfc3339(&e.occurred_at)
                    .unwrap()
                    .with_timezone(&Utc),
            })
            .collect()
    }
}

/// How a scripted candidate ends.
#[derive(Clone, Copy)]
struct Fate {
    completes: bool,
    eligible: bool,
}

/// A whole round: created, run, validated, judged (with `judge_fails`
/// failed attempts first), then a winner or a rejection, and an optional
/// promotion.
fn decide(fates: &[Fate], judge_fails: u32, promote: bool) -> (Script, FinalOutcome) {
    let labels: Vec<String> = (0..fates.len())
        .map(|i| ((b'A' + i as u8) as char).to_string())
        .collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut s = Script::new();
    s.push(
        EventKind::ExperimentCreated(experiment_created(fates.len() as u32)),
        None,
    );
    s.push(
        EventKind::PreflightCompleted(PreflightCompleted {
            report: preflight_report(),
        }),
        None,
    );
    s.push(EventKind::RoundCreated(round_created(&refs)), None);
    for l in &refs {
        s.push(EventKind::CandidatePlanned(candidate_planned(l)), Some(l));
    }
    for l in &refs {
        s.push(
            EventKind::WorktreeCreated(WorktreeCreated {
                label: l.to_string(),
                path: format!("/wt/{l}").into(),
                branch: format!("mh/exp/0199a5b0/r0/{l}"),
                base_sha: "a".repeat(40),
            }),
            Some(l),
        );
    }
    for l in &refs {
        s.push(
            EventKind::CandidateSpawned(CandidateSpawned {
                label: l.to_string(),
                pane: PaneId::new(format!("p-{l}")).unwrap(),
                routing: routing("sonnet"),
            }),
            Some(l),
        );
    }
    for (l, f) in refs.iter().zip(fates) {
        let kind = if f.completes {
            EventKind::CandidateCompleted(CandidateCompleted {
                label: l.to_string(),
                exit_code: Some(0),
            })
        } else {
            EventKind::CandidateFailed(CandidateFailed {
                label: l.to_string(),
                failure: FailureKind::TimedOut,
            })
        };
        s.push(kind, Some(l));
    }
    for (l, f) in refs.iter().zip(fates) {
        s.push(
            EventKind::CandidateFrozen(CandidateFrozen {
                label: l.to_string(),
                head_sha: "b".repeat(40),
                numstat: numstat(),
                diff_digest: dg(l),
            }),
            Some(l),
        );
        s.push(
            EventKind::ValidationCompleted(ValidationCompleted {
                label: l.to_string(),
                report: validation_report(l, f.eligible),
            }),
            Some(l),
        );
    }
    let Some(winner) = refs
        .iter()
        .zip(fates)
        .find(|(_, f)| f.eligible)
        .map(|(l, _)| *l)
    else {
        s.push(
            EventKind::WinnerRejected(WinnerRejected {
                reason: RejectReason::NoEligible,
            }),
            None,
        );
        return (s, FinalOutcome::Rejected);
    };
    for attempt in 1..=judge_fails + 1 {
        s.push(
            EventKind::JudgeScheduled(JudgeScheduled {
                attempt,
                input_digest: dg("input"),
                judge_policy_digest: dg("policy"),
                job_dir: format!("/jobs/r/judge-{attempt}").into(),
            }),
            None,
        );
        s.push(
            EventKind::JudgeStarted(JudgeStarted {
                attempt,
                pid: 4000 + attempt,
            }),
            None,
        );
        if attempt <= judge_fails {
            s.push(
                EventKind::JudgeFailed(JudgeFailed {
                    attempt,
                    cause: JudgeFailure::TimedOut,
                }),
                None,
            );
            if attempt >= 2 {
                return (s, FinalOutcome::NeedsIntervention);
            }
        }
    }
    let judgment = JudgmentId::new("j-1").unwrap();
    s.push(
        EventKind::JudgeCompleted(JudgeCompleted {
            attempt: judge_fails + 1,
            judgment_id: judgment.clone(),
            output_digest: dg("out"),
        }),
        None,
    );
    s.push(
        EventKind::WinnerSelected(WinnerSelected {
            label: winner.to_string(),
            execution_id: exec_id(winner),
            head_sha: "b".repeat(40),
            judgment_id: judgment,
            promotion: if promote {
                PromotionIntent::Requested {
                    target: "main".into(),
                }
            } else {
                PromotionIntent::NotRequested
            },
        }),
        Some(winner),
    );
    if promote {
        s.push(
            EventKind::PromotionStarted(PromotionStarted {
                target: "main".into(),
                dest_before: "a".repeat(40),
                planned_after: "b".repeat(40),
                strategy: horch_core::competition::promotion::PromotionStrategy::FastForward,
                publish: "update_ref_cas".into(),
                validation_ids: vec!["v-1".into()],
            }),
            None,
        );
        s.push(
            EventKind::PromotionCompleted(PromotionCompleted {
                receipt_digest: dg("receipt"),
                dest_after: "b".repeat(40),
            }),
            None,
        );
    }
    let outcome = if promote {
        FinalOutcome::Promoted
    } else {
        FinalOutcome::Winner
    };
    (s, outcome)
}

/// [`decide`], then cleanup and completion.
fn lifecycle(fates: &[Fate], judge_fails: u32, promote: bool) -> Script {
    let (mut s, final_outcome) = decide(fates, judge_fails, promote);
    s.push(EventKind::RoundCleanupStarted(RoundCleanupStarted {}), None);
    s.push(
        EventKind::RoundCompleted(RoundCompleted { final_outcome }),
        None,
    );
    s
}

/// A random lifecycle, then random drops, repeats, swaps and foreign
/// events, so the fold sees both valid and invalid transitions.
fn random_events(rng: &mut SplitMix64) -> Vec<EventEnvelope> {
    // Every (fates, judge failures, promote) script, built once: building
    // payloads costs more than folding them.
    static SCRIPTS: std::sync::OnceLock<Vec<Vec<EventEnvelope>>> = std::sync::OnceLock::new();
    let scripts = SCRIPTS.get_or_init(|| {
        let mut all = Vec::new();
        for n in 1..=2usize {
            for bits in 0..(1u32 << (2 * n)) {
                let fates: Vec<Fate> = (0..n)
                    .map(|i| Fate {
                        completes: bits >> (2 * i) & 1 == 1,
                        eligible: bits >> (2 * i + 1) & 1 == 1,
                    })
                    .collect();
                for judge_fails in 0..3 {
                    for promote in [false, true] {
                        all.push(lifecycle(&fates, judge_fails, promote).events);
                    }
                }
            }
        }
        all
    });
    let mut events = scripts[rng.below(scripts.len() as u64) as usize].clone();
    for _ in 0..rng.below(4) {
        if events.is_empty() {
            break;
        }
        let i = rng.below(events.len() as u64) as usize;
        match rng.below(5) {
            0 => {
                events.remove(i);
            }
            1 => {
                let e = events[i].clone();
                events.insert(i, e);
            }
            2 if i + 1 < events.len() => events.swap(i, i + 1),
            3 => {
                let mut e = events[i].clone();
                e.kind = "candidate.teleported".into();
                events.insert(i, e);
            }
            _ => {
                let mut e = events[i].clone();
                e.payload = json!({"garbage": true});
                events.insert(i, e);
            }
        }
    }
    events
}

#[test]
fn mea_05_full_lifecycles_fold_without_anomalies() {
    let ok = Fate {
        completes: true,
        eligible: true,
    };
    let bad = Fate {
        completes: false,
        eligible: false,
    };
    let cases = vec![
        (
            (vec![ok, bad], 0, false),
            RoundState::Decided,
            FinalOutcome::Winner,
        ),
        (
            (vec![ok], 1, true),
            RoundState::Promoted,
            FinalOutcome::Promoted,
        ),
        (
            (vec![ok, ok], 2, false),
            RoundState::NeedsIntervention,
            FinalOutcome::NeedsIntervention,
        ),
        (
            (vec![bad, bad], 0, false),
            RoundState::Rejected,
            FinalOutcome::Rejected,
        ),
    ];
    for ((fates, fails, promote), decided, outcome) in cases {
        let (s, _) = decide(&fates, fails, promote);
        let p = fold(&s.events);
        assert_eq!(p.anomalies, vec![]);
        assert_eq!(p.rounds[&round_id()].state, decided);

        let s = lifecycle(&fates, fails, promote);
        let p = fold(&s.events);
        assert_eq!(p.anomalies, vec![]);
        let r = &p.rounds[&round_id()];
        assert_eq!(r.state, RoundState::Complete);
        assert_eq!(r.cleaned_from, Some(decided));
        assert_eq!(r.final_outcome, Some(outcome));
        assert_eq!(p.experiments[&exp_id()].state, RoundState::Planned);
    }
}

#[test]
fn mea_05_round_needs_intervention_and_completion_checks() {
    let ok = Fate {
        completes: true,
        eligible: true,
    };
    // The judge answered, but the winner policy found no clear winner.
    let (mut s, _) = decide(&[ok, ok], 0, false);
    s.events.pop(); // winner.selected
    s.push(
        EventKind::RoundNeedsIntervention(RoundNeedsIntervention {
            reason: "tie".into(),
            source: InterventionSource::Judge,
        }),
        None,
    );
    let p = fold(&s.events);
    assert_eq!(p.anomalies, vec![]);
    assert_eq!(p.rounds[&round_id()].state, RoundState::NeedsIntervention);

    // A completion that contradicts the state cleanup started from.
    s.push(EventKind::RoundCleanupStarted(RoundCleanupStarted {}), None);
    s.push(
        EventKind::RoundCompleted(RoundCompleted {
            final_outcome: FinalOutcome::Winner,
        }),
        None,
    );
    let p = fold(&s.events);
    assert_eq!(p.anomalies.len(), 1, "{:?}", p.anomalies);
    assert_eq!(p.rounds[&round_id()].state, RoundState::Cleanup);

    // The promotion source is invalid before any promotion.
    let (mut s, _) = decide(&[ok], 0, false);
    s.push(
        EventKind::RoundNeedsIntervention(RoundNeedsIntervention {
            reason: "dirty".into(),
            source: InterventionSource::Promotion,
        }),
        None,
    );
    let p = fold(&s.events);
    assert_eq!(p.anomalies.len(), 1);
    assert_eq!(p.rounds[&round_id()].state, RoundState::Decided);
}

#[test]
fn mea_05_invalid_transition_is_an_anomaly_and_not_applied() {
    let mut s = Script::new();
    s.push(EventKind::ExperimentCreated(experiment_created(1)), None);
    s.push(
        EventKind::PreflightCompleted(PreflightCompleted {
            report: preflight_report(),
        }),
        None,
    );
    s.push(EventKind::RoundCreated(round_created(&["A"])), None);
    let before = fold(&s.events);
    s.push(
        EventKind::PromotionCompleted(PromotionCompleted {
            receipt_digest: dg("receipt"),
            dest_after: "b".repeat(40),
        }),
        None,
    );
    let after = fold(&s.events);
    assert_eq!(after.anomalies.len(), 1);
    assert_eq!(after.anomalies[0].event_id.as_str(), "evt-00003");
    assert!(
        after.anomalies[0].reason.contains("PLANNED"),
        "{:?}",
        after.anomalies
    );
    assert_eq!(after.rounds, before.rounds);
    assert_eq!(after.experiments, before.experiments);
}

#[test]
fn mea_05_prop_fold_deterministic() {
    property(0x6d65_615f_3035, 10_000, |rng| {
        let events = random_events(rng);
        let a = fold(&events);
        let b = fold(&events);
        assert_eq!(a, b);
        assert!(a.anomalies.len() <= events.len());
    });
}

#[test]
fn mea_05_prop_replay_identical() {
    property(0x6d65_615f_3036, 10_000, |rng| {
        let events = random_events(rng);
        let split = rng.below(events.len() as u64 + 1) as usize;
        let mut p: Projection = fold(&events[..split]);
        for e in &events[split..] {
            p.apply(e);
        }
        assert_eq!(p, fold(&events));
    });
}

#[test]
fn mea_05_rebuild_equals_live() {
    let ok = Fate {
        completes: true,
        eligible: true,
    };
    let script = lifecycle(&[ok, ok], 1, true);
    let tmp = tempfile::tempdir().unwrap();
    let (_, rec) = open_dataset(tmp.path());
    let mut live = Vec::new();
    for e in script.new_events() {
        match rec.append(e).unwrap() {
            Appended::Recorded(env) => live.push(env),
            other => panic!("{other:?}"),
        }
    }
    let live_p = fold(&live);
    assert_eq!(live_p.anomalies, vec![]);
    drop(rec);
    let (_, rec) = open_dataset(tmp.path());
    let read = rec.read_all().unwrap();
    assert_eq!(read.torn_lines, 0);
    assert_eq!(read.events, live);
    assert_eq!(fold(&read.events), live_p);
}

// ─── SEC-02: the environment snapshot ───────────────────────────────────────

#[test]
fn sec_02_env_snapshot_allowlist() {
    use horch_core::measure::envsnap::env_snapshot;
    let vars: std::collections::BTreeMap<String, String> = [
        ("ANTHROPIC_API_KEY", "sk-ant-api03-abcdefghijklmnop"),
        ("OPENAI_API_KEY", "sk-abcdefghijklmnopqrstuvwxyz"),
        ("AWS_SECRET_ACCESS_KEY", "wJalrXUtnFEMI/K7MDENG"),
        ("GITHUB_TOKEN", "ghp_abcdefghijklmnopqrstuvwxyz"),
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/Users/someone"),
        ("LANG", "en_US.UTF-8"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let snap = env_snapshot(&vars);
    assert_eq!(
        snap.into_iter().collect::<Vec<_>>(),
        vec![("LANG".to_string(), "en_US.UTF-8".to_string())]
    );
}

#[test]
fn sec_02_env_snapshot_presence_and_redaction() {
    use horch_core::measure::envsnap::env_snapshot;
    let vars: std::collections::BTreeMap<String, String> = [
        ("HORCH_TEAMMATES_DIR", "/Users/someone/teammates"),
        ("HORCH_CLAUDE_BIN", "/opt/claude"),
        ("HORCH__BIN", "x"),
        ("HORCH_BALANCE", "token=sk-ant-api03-abcdefghijklmnop"),
        ("TZ", "UTC"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let snap = env_snapshot(&vars);
    assert_eq!(snap["HORCH_TEAMMATES_DIR"], "set");
    assert_eq!(snap["HORCH_CLAUDE_BIN"], "set");
    assert!(!snap.contains_key("HORCH__BIN"));
    assert_eq!(snap["TZ"], "UTC");
    assert!(!snap["HORCH_BALANCE"].contains("sk-ant"), "{snap:?}");
    assert_eq!(snap.len(), 4);
}

// ─── MEA-06, MEA-11: WorkerRun 1.0.0 ────────────────────────────────────────

use horch_core::execution::ExecutionStatus;
use horch_core::measure::digest::canonical_json;
use horch_core::measure::worker_run::{ExecutionFacts, WorkerRun};
use horch_core::usage::money::{CostSource, MicroUsd};
use horch_core::usage::Tokens;

/// One candidate, planned → spawned → completed → frozen → validated.
fn one_candidate_lifecycle() -> Script {
    let mut s = Script::new();
    s.push(EventKind::ExperimentCreated(experiment_created(1)), None);
    s.push(
        EventKind::PreflightCompleted(PreflightCompleted {
            report: preflight_report(),
        }),
        None,
    );
    s.push(EventKind::RoundCreated(round_created(&["A"])), None);
    s.push(
        EventKind::CandidatePlanned(candidate_planned("A")),
        Some("A"),
    );
    s.push(
        EventKind::WorktreeCreated(WorktreeCreated {
            label: "A".into(),
            path: "/wt/A".into(),
            branch: "mh/exp/0199a5b0/r0/A".into(),
            base_sha: "a".repeat(40),
        }),
        Some("A"),
    );
    s.push(
        EventKind::CandidateSpawned(CandidateSpawned {
            label: "A".into(),
            pane: PaneId::new("p-A").unwrap(),
            routing: routing("sonnet"),
        }),
        Some("A"),
    );
    s.push(
        EventKind::CandidateCompleted(CandidateCompleted {
            label: "A".into(),
            exit_code: Some(0),
        }),
        Some("A"),
    );
    s.push(
        EventKind::CandidateFrozen(CandidateFrozen {
            label: "A".into(),
            head_sha: "b".repeat(40),
            numstat: numstat(),
            diff_digest: dg("A"),
        }),
        Some("A"),
    );
    s.push(
        EventKind::ValidationCompleted(ValidationCompleted {
            label: "A".into(),
            report: validation_report("A", true),
        }),
        Some("A"),
    );
    s
}

fn execution_facts() -> ExecutionFacts {
    ExecutionFacts {
        execution_id: exec_id("A"),
        status: ExecutionStatus::Done,
        session_id: Some(
            horch_core::ids::SessionId::new("5d0c6c1e-0000-4000-8000-00000000000a").unwrap(),
        ),
        transcript_ref: Some("~/.claude/projects/-wt-A/5d0c6c1e.jsonl".into()),
        transcript_digest: Some(dg("transcript")),
        started_at: "2026-10-02T12:00:01.000Z".into(),
        finished_at: Some("2026-10-02T12:07:31.250Z".into()),
        tokens: Tokens {
            input: 1200,
            cache_write_5m: 3000,
            cache_write_1h: 0,
            cache_read: 45000,
            output: 2100,
        },
        cost_microusd: MicroUsd(123_456),
        cost_source: CostSource::PriceTable {
            date: "2026-09-28".into(),
        },
        skills: typed(json!([{
            "id": "tdd", "version": "bundled+0123456789ab",
            "digest": dg("skill:tdd").to_string(), "source": "bundled", "policy": "explicit",
        }])),
        routing: None,
    }
}

fn judge_components() -> Option<std::collections::BTreeMap<String, f64>> {
    Some(
        [("correctness", 0.9), ("scope", 0.75), ("tests", 1.0)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

/// Sorted keys, pretty, `\n`-terminated: the golden file's form.
fn golden_json(run: &WorkerRun) -> String {
    let canonical = canonical_json(&serde_json::to_value(run).unwrap());
    let value: Value = serde_json::from_str(&canonical).unwrap();
    serde_json::to_string_pretty(&value).unwrap() + "\n"
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

fn sorted(list: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

#[test]
fn mea_06_worker_run_schema() {
    let s = one_candidate_lifecycle();
    let run = WorkerRun::project(&execution_facts(), &s.events, judge_components()).unwrap();
    let v = serde_json::to_value(&run).unwrap();
    assert_eq!(
        keys(&v),
        sorted(&[
            "schema_version",
            "worker_run_id",
            "experiment_id",
            "round_id",
            "label",
            "task_id",
            "task_digest",
            "config",
            "facts",
            "scores",
        ])
    );
    assert_eq!(
        keys(&v["config"]),
        sorted(&[
            "config_id",
            "teammate",
            "harness",
            "model",
            "effort",
            "slot",
            "propensity",
            "skills",
            "routing",
        ])
    );
    assert_eq!(
        keys(&v["facts"]),
        sorted(&[
            "status",
            "base_sha",
            "head_sha",
            "numstat",
            "diff_digest",
            "session_id",
            "transcript_ref",
            "transcript_digest",
            "started_at",
            "finished_at",
            "latency_ms",
            "tokens",
            "cost_microusd",
            "cost_source",
            "environment_digest",
        ])
    );
    assert_eq!(
        keys(&v["scores"]),
        sorted(&["gates", "mechanical_score", "judge_components"])
    );
    assert_eq!(v["schema_version"], "1.0.0");
    assert_eq!(v["worker_run_id"], exec_id("A").as_str());
    assert_eq!(v["facts"]["latency_ms"], 450_250);
    assert_eq!(v["facts"]["cost_source"], "price_table@2026-09-28");
    let back: WorkerRun = serde_json::from_value(v).unwrap();
    assert_eq!(back, run);
}

#[test]
fn mea_06_no_winner_field() {
    fn walk(v: &Value, path: &str) {
        match v {
            Value::Object(m) => {
                for (k, child) in m {
                    assert!(!k.contains("winner"), "{path}.{k}");
                    walk(child, &format!("{path}.{k}"));
                }
            }
            Value::Array(items) => items.iter().for_each(|c| walk(c, path)),
            _ => {}
        }
    }
    let s = one_candidate_lifecycle();
    let run = WorkerRun::project(&execution_facts(), &s.events, judge_components()).unwrap();
    let mut v = serde_json::to_value(&run).unwrap();
    walk(&v, "$");
    // Facts and scores stay apart: no score inside facts.
    assert!(v["facts"].get("mechanical_score").is_none());
    assert!(v["facts"].get("gates").is_none());
    v.as_object_mut()
        .unwrap()
        .insert("winner".into(), json!(true));
    assert!(serde_json::from_value::<WorkerRun>(v).is_err());
}

#[test]
fn mea_11_fake_lifecycle_replays_identical_worker_run() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/worker-run-1.0.0.json");
    let tmp = tempfile::tempdir().unwrap();
    let (_, rec) = open_dataset(tmp.path());
    for e in one_candidate_lifecycle().new_events() {
        assert!(matches!(rec.append(e).unwrap(), Appended::Recorded(_)));
    }
    drop(rec);
    let (_, rec) = open_dataset(tmp.path());
    let read = rec.read_all().unwrap();
    assert_eq!(read.torn_lines, 0);
    let run = WorkerRun::project(&execution_facts(), &read.events, judge_components()).unwrap();
    let actual = golden_json(&run);

    if std::env::var("HORCH_BLESS").ok().as_deref() == Some("1") && !golden.exists() {
        std::fs::write(&golden, &actual).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&golden).unwrap_or_else(|e| {
        panic!(
            "{}: {e}. The WorkerRun 1.0.0 golden is frozen; it is blessed once only",
            golden.display()
        )
    });
    assert_eq!(
        actual, want,
        "WorkerRun 1.0.0 changed. Never re-bless: a format change bumps schema_version"
    );
}

// ─── CMP-02: the domain has no adapters ─────────────────────────────────────

#[test]
fn cmp_02_domain_has_no_adapter_imports() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let forbidden = [
        "std::process",
        "Command",
        "herdr",
        "workspace::",
        "launch::",
        "vcs::",
        "std::env",
        "std::fs",
    ];
    // store.rs and recorder.rs are adapters and are exempt.
    for file in [
        "competition/model.rs",
        "measure/event.rs",
        "measure/projection.rs",
        "measure/worker_run.rs",
    ] {
        let text = std::fs::read_to_string(src.join(file)).unwrap();
        for word in forbidden {
            assert!(!text.contains(word), "{file} contains {word:?}");
        }
    }
}

#[test]
fn candidate_labels_are_anonymous_letters() {
    use horch_core::competition::model::CandidateLabel;
    let got: Vec<String> = [0, 1, 25, 26, 27, 701, 702]
        .into_iter()
        .map(|i| CandidateLabel::from_index(i).to_string())
        .collect();
    assert_eq!(got, ["A", "B", "Z", "AA", "AB", "ZZ", "AAA"]);
    for bad in ["", "a", "A1", "sonnet"] {
        assert!(CandidateLabel::new(bad).is_err(), "{bad:?}");
    }
    assert!(serde_json::from_str::<CandidateLabel>(r#""opus""#).is_err());
    assert_eq!(
        serde_json::to_string(&CandidateLabel::new("C").unwrap()).unwrap(),
        r#""C""#
    );
}
