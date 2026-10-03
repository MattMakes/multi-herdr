//! B1 measurement layer: dataset paths, the event log, the projection fold
//! and WorkerRun 1.0.0 (dataset design §2 and §4.1 to §4.3).

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use horch_core::execution::FailureKind;
use horch_core::harness::HarnessKind;
use horch_core::ids::{
    EventId, ExecutionId, ExperimentId, JudgmentId, ModelId, PaneId, RoundId, TaskId, TeammateName,
};
use horch_core::measure::digest::{sha256_bytes, Digest};
use horch_core::measure::event::*;
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::recorder::{Appended, JsonlRecorder, NewEvent, NoopRecorder, Recorder};
use horch_core::measure::store::{EventIndex, StoreOptions};
use horch_core::measure::testkit::property;
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
        eligible_set: vec![json!({"teammate": "sonnet", "verdict": "eligible"})],
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

fn validation_report(label: &str, eligible: bool) -> Value {
    json!({
        "validation_id": "0199a5b0-0000-7000-8000-0000000000f1",
        "label": label,
        "head_sha": "b".repeat(40),
        "gates": [{"name": "test", "status": {"status": "passed"}, "duration_ms": 1200,
                   "log_ref": "validation/A/test.log", "log_digest": dg("log").to_string(),
                   "log_truncated": false}],
        "mechanical_score": if eligible { 1.0 } else { 0.0 },
        "eligible": eligible,
    })
}

/// One sample of every known kind, in `EventKind::KNOWN` order.
fn sample_kinds() -> Vec<EventKind> {
    vec![
        EventKind::ExperimentCreated(experiment_created(2)),
        EventKind::PreflightCompleted(PreflightCompleted {
            report: json!({"passed": true, "checks": []}),
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
            routing: json!({"requested": "sonnet", "resolved": "sonnet"}),
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
            numstat: vec![json!({"added": 3, "removed": 1, "path": "src/lib.rs"})],
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
            reason: json!("no_eligible"),
        }),
        EventKind::PromotionStarted(PromotionStarted {
            target: "main".into(),
            dest_before: "a".repeat(40),
            planned_after: "b".repeat(40),
            strategy: json!("fast_forward"),
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
    std::fs::create_dir_all(paths.experiment_dir(&exp)).unwrap();
    // Valid-looking ledgers at every dataset level, plus an event file.
    for p in [
        paths.root().join("ledger.json"),
        paths.root().parent().unwrap().join("ledger.json"),
        paths.manifest(&exp),
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
            paths.manifest(&exp),
            "/s/multi-herdr/p/experiments/e/manifest.json",
        ),
        (
            paths.round_file(&exp, &round),
            "/s/multi-herdr/p/experiments/e/rounds/r.json",
        ),
        (
            paths.judge_input_dir(&exp, &round),
            "/s/multi-herdr/p/experiments/e/artifacts/r/judge-input",
        ),
        (
            paths.validation_dir(&exp, &round, "A"),
            "/s/multi-herdr/p/experiments/e/artifacts/r/validation/A",
        ),
        (
            paths.judgement(&round),
            "/s/multi-herdr/p/judgements/r.json",
        ),
        (
            paths.promotion(&round),
            "/s/multi-herdr/p/promotions/r.json",
        ),
        (paths.exports_dir("lp-1"), "/s/multi-herdr/p/exports/lp-1"),
        (paths.job_dir(&round, 2), "/s/multi-herdr/p/jobs/r/judge-2"),
        (
            paths.default_worktree_root(&exp),
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
