//! B6 export, readiness and outcomes (dataset design §4.10, EXP-03 to
//! EXP-06).
//!
//! The fixture is generated through `JsonlRecorder` with fixed ids and
//! times: 2 experiments, 3 rounds. Round 1 has a winner, a judgment and an
//! outcome; round 2 is a judged tie (needs intervention); round 3 has no
//! eligible candidate (rejected). Event ids are fresh on every build, and the
//! export never shows them.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Duration, TimeZone, Utc};
use horch_core::competition::preflight::PreflightReport;
use horch_core::dataset::export::{
    export, render_jsonl, write_export, ExportRow, InMemoryFacts, EXPORT_SCHEMA,
};
use horch_core::dataset::outcome::record_outcome;
use horch_core::dataset::readiness::{readiness, ReadinessThresholds, ReadinessVerdict};
use horch_core::evaluation::judgment::{
    CandidateAssessment, Judgment, JudgmentRecord, JudgmentVerdict,
};
use horch_core::evaluation::validator::ValidationReport;
use horch_core::execution::{ExecutionStatus, FailureKind};
use horch_core::harness::HarnessKind;
use horch_core::ids::{
    ExecutionId, ExperimentId, JudgmentId, ModelId, PaneId, RoundId, TaskId, TeammateName,
};
use horch_core::measure::digest::{sha256_bytes, Digest};
use horch_core::measure::event::*;
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::recorder::{Appended, JsonlRecorder, NewEvent, Recorder};
use horch_core::measure::store::StoreOptions;
use horch_core::measure::worker_run::ExecutionFacts;
use horch_core::teacher::TeacherRef;
use horch_core::usage::money::{CostSource, MicroUsd};
use horch_core::usage::Tokens;
use serde_json::{json, Value};

const LPV: &str = "lp-1";

// ─── fixture ────────────────────────────────────────────────────────────────

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 2, 12, 0, 0).unwrap()
}

fn dg(tag: &str) -> Digest {
    sha256_bytes(tag.as_bytes())
}

fn uuid(n: u64) -> String {
    format!("0199a5b0-0000-7000-8000-{n:012x}")
}

fn exp(n: u64) -> ExperimentId {
    ExperimentId::new(uuid(0xe00 + n)).unwrap()
}

fn round(n: u64) -> RoundId {
    RoundId::new(uuid(0xa00 + n)).unwrap()
}

fn exec(round: u64, label: &str) -> ExecutionId {
    let l = u64::from(label.as_bytes()[0]);
    ExecutionId::new(uuid(0xc000 + round * 0x100 + l)).unwrap()
}

fn judgment_id(round: u64) -> JudgmentId {
    JudgmentId::new(uuid(0xd00 + round)).unwrap()
}

fn typed<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}

/// Sonnet and codex-sol: the two arms of every fixture round.
struct Arm {
    teammate: &'static str,
    harness: HarnessKind,
    model: &'static str,
    effort: Option<&'static str>,
    config_id: &'static str,
}

const ARMS: [Arm; 2] = [
    Arm {
        teammate: "sonnet",
        harness: HarnessKind::Claude,
        model: "sonnet",
        effort: Some("high"),
        config_id: "sonnet|claude|sonnet|high",
    },
    Arm {
        teammate: "codex-sol",
        harness: HarnessKind::Codex,
        model: "gpt-5-codex",
        effort: None,
        config_id: "codex-sol|codex|gpt-5-codex|-",
    },
];

fn experiment_created(task: &str) -> ExperimentCreated {
    ExperimentCreated {
        task_id: TaskId::new(task).unwrap(),
        task_digest: dg(task),
        config_digest: dg("config"),
        base_sha: "a".repeat(40),
        repo_digest: dg("repo"),
        environment_digest: dg("env"),
        candidates: 2,
        strategy: "baseline+diversity".into(),
        budget_usd_micro: 5_000_000,
        promote_to: None,
    }
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

fn round_created(index: u32) -> RoundCreated {
    RoundCreated {
        index,
        base_sha: "a".repeat(40),
        labels: vec!["A".into(), "B".into()],
        eligible_set: typed(json!([
            {"teammate": "sonnet", "harness": "claude", "model": "sonnet", "effort": "high",
             "fallback_index": null, "pool": "claude-max", "pool_state": "ok",
             "verdict": "eligible"},
            {"teammate": "codex-sol", "harness": "codex", "model": "gpt-5-codex", "effort": null,
             "fallback_index": null, "pool": "codex", "pool_state": "ok",
             "verdict": "eligible"},
            {"teammate": "opus", "harness": "claude", "model": "opus", "effort": "high",
             "fallback_index": null, "pool": "claude-max", "pool_state": "ok",
             "verdict": "excluded", "reason": "reserved_tier"},
        ])),
        propensities: BTreeMap::from([("A".into(), 1.0), ("B".into(), 0.5)]),
        teacher: TeacherRef::none(),
        seed: 42,
        label_policy_version: LPV.into(),
    }
}

fn planned(label: &str, arm: &Arm, propensity: f64) -> CandidatePlanned {
    CandidatePlanned {
        label: label.into(),
        teammate: TeammateName::new(arm.teammate).unwrap(),
        harness: arm.harness,
        model: ModelId::new(arm.model).unwrap(),
        effort: arm.effort.map(str::to_string),
        slot: if propensity == 1.0 {
            SlotKind::Baseline
        } else {
            SlotKind::Exploration
        },
        propensity,
        config_id: arm.config_id.into(),
    }
}

fn validation_report(label: &str, eligible: bool) -> ValidationReport {
    let status = if eligible {
        json!({"kind": "passed"})
    } else {
        json!({"kind": "failed", "code": 101})
    };
    typed(json!({
        "validation_id": uuid(0xf00 + u64::from(label.as_bytes()[0])),
        "label": label,
        "head_sha": "b".repeat(40),
        "gates": [
            {"name": "build", "status": {"kind": "passed"}, "duration_ms": 900,
             "log_ref": format!("validation/{label}/build.log"),
             "log_digest": dg("build-log").to_string(), "log_truncated": false},
            {"name": "test", "status": status, "duration_ms": 1200,
             "log_ref": format!("validation/{label}/test.log"),
             "log_digest": dg("test-log").to_string(), "log_truncated": false},
        ],
        "mechanical_score": if eligible { 1.0 } else { 0.5 },
        "eligible": eligible,
    }))
}

fn routing(teammate: &str, pool: &str) -> Value {
    json!({
        "requested": teammate, "resolved": teammate, "fallback_index": null,
        "pool": pool, "pool_state": "ok", "reason": null, "mode": "pinned",
    })
}

fn scores(correctness: f64, scope: f64, tests: f64) -> BTreeMap<String, f64> {
    [
        ("correctness", correctness),
        ("scope", scope),
        ("tests", tests),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Appends events at 1-second steps from a start time.
struct Log<'a> {
    rec: &'a JsonlRecorder,
    exp: ExperimentId,
    round: Option<RoundId>,
    at: DateTime<Utc>,
    n: u32,
}

impl Log<'_> {
    fn push(&mut self, kind: EventKind, execution: Option<ExecutionId>) {
        self.n += 1;
        self.at += Duration::seconds(1);
        let key = format!("{}:{}:{}", kind.name(), self.exp, self.n);
        let got = self
            .rec
            .append(NewEvent {
                kind,
                actor: Actor::Coordinator,
                experiment_id: self.exp.clone(),
                round_id: self.round.clone(),
                execution_id: execution,
                idempotency_key: key,
                occurred_at: self.at,
            })
            .unwrap();
        assert!(matches!(got, Appended::Recorded(_)));
    }
}

/// How each candidate of a round ends.
#[derive(Clone, Copy)]
enum Fate {
    Eligible,
    FailedGates,
    Crashed,
}

/// Plan, provision, run and validate both candidates of round `n`.
fn run_round(log: &mut Log, n: u64, index: u32, fates: [Fate; 2]) {
    log.round = Some(round(n));
    log.push(EventKind::RoundCreated(round_created(index)), None);
    for (label, (arm, p)) in ["A", "B"].iter().zip(ARMS.iter().zip([1.0, 0.5])) {
        log.push(
            EventKind::CandidatePlanned(planned(label, arm, p)),
            Some(exec(n, label)),
        );
    }
    for label in ["A", "B"] {
        log.push(
            EventKind::WorktreeCreated(WorktreeCreated {
                label: label.into(),
                path: format!("/wt/{n}/{label}").into(),
                branch: format!("mh/r{n}/{label}"),
                base_sha: "a".repeat(40),
            }),
            Some(exec(n, label)),
        );
    }
    for ((label, arm), fate) in ["A", "B"].iter().zip(&ARMS).zip(fates) {
        let pool = if arm.harness == HarnessKind::Codex {
            "codex"
        } else {
            "claude-max"
        };
        log.push(
            EventKind::CandidateSpawned(CandidateSpawned {
                label: label.to_string(),
                pane: PaneId::new(format!("p-{n}-{label}")).unwrap(),
                routing: typed(routing(arm.teammate, pool)),
            }),
            Some(exec(n, label)),
        );
        let end = match fate {
            Fate::Crashed => EventKind::CandidateFailed(CandidateFailed {
                label: label.to_string(),
                failure: FailureKind::AgentExited { code: Some(1) },
            }),
            _ => EventKind::CandidateCompleted(CandidateCompleted {
                label: label.to_string(),
                exit_code: Some(0),
            }),
        };
        log.push(end, Some(exec(n, label)));
    }
    for (label, fate) in ["A", "B"].iter().zip(fates) {
        log.push(
            EventKind::CandidateFrozen(CandidateFrozen {
                label: label.to_string(),
                head_sha: "b".repeat(40),
                numstat: typed(json!([{"added": 3, "deleted": 1, "path": "src/lib.rs"}])),
                diff_digest: dg(&format!("diff-{n}-{label}")),
            }),
            Some(exec(n, label)),
        );
        log.push(
            EventKind::ValidationCompleted(ValidationCompleted {
                label: label.to_string(),
                report: validation_report(label, matches!(fate, Fate::Eligible)),
            }),
            Some(exec(n, label)),
        );
    }
}

fn judge(log: &mut Log, paths: &DatasetPaths, n: u64, judgment: Judgment) {
    log.push(
        EventKind::JudgeScheduled(JudgeScheduled {
            attempt: 1,
            input_digest: dg("input"),
            judge_policy_digest: dg("policy"),
            job_dir: format!("/jobs/{n}/judge-1").into(),
        }),
        None,
    );
    log.push(
        EventKind::JudgeStarted(JudgeStarted {
            attempt: 1,
            pid: 4242,
        }),
        None,
    );
    log.push(
        EventKind::JudgeCompleted(JudgeCompleted {
            attempt: 1,
            judgment_id: judgment_id(n),
            output_digest: dg("output"),
        }),
        None,
    );
    let record = JudgmentRecord {
        judgment_id: judgment_id(n),
        round_id: round(n),
        attempt: 1,
        input_digest: dg("input"),
        judge_policy_digest: dg("policy"),
        execution_id: ExecutionId::new(uuid(0xb00 + n)).unwrap(),
        judgment,
    };
    std::fs::write(
        paths.judgement(&round(n)).unwrap(),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
}

fn assessment(s: BTreeMap<String, f64>) -> CandidateAssessment {
    CandidateAssessment {
        scores: s,
        acceptable: true,
        notes: "ok".into(),
    }
}

/// The time of round 1's outcome.
fn outcome_at() -> DateTime<Utc> {
    t0() + Duration::hours(2)
}

fn build_fixture(state: &Path) -> (DatasetPaths, JsonlRecorder, InMemoryFacts) {
    let paths = DatasetPaths::from_slug(state, "fixture");
    let rec = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();

    // Experiment 2 comes first in the log; the export still sorts it last.
    let mut log = Log {
        rec: &rec,
        exp: exp(2),
        round: None,
        at: t0(),
        n: 0,
    };
    log.push(
        EventKind::ExperimentCreated(experiment_created("task-two")),
        None,
    );
    log.push(
        EventKind::PreflightCompleted(PreflightCompleted {
            report: preflight_report(),
        }),
        None,
    );
    // Round 3: every candidate fails; no judge.
    run_round(&mut log, 3, 0, [Fate::Crashed, Fate::FailedGates]);
    log.push(
        EventKind::WinnerRejected(WinnerRejected {
            reason: typed(json!("no_eligible")),
        }),
        None,
    );

    let mut log = Log {
        rec: &rec,
        exp: exp(1),
        round: None,
        at: t0() + Duration::minutes(10),
        n: 0,
    };
    log.push(
        EventKind::ExperimentCreated(experiment_created("task-one")),
        None,
    );
    log.push(
        EventKind::PreflightCompleted(PreflightCompleted {
            report: preflight_report(),
        }),
        None,
    );
    // Round 1: A wins.
    run_round(&mut log, 1, 0, [Fate::Eligible, Fate::Eligible]);
    judge(
        &mut log,
        &paths,
        1,
        Judgment {
            schema_version: "1.0.0".into(),
            verdict: JudgmentVerdict::Winner,
            winner: Some("A".into()),
            ranking: vec!["A".into(), "B".into()],
            candidates: BTreeMap::from([
                ("A".into(), assessment(scores(9.0, 8.0, 7.5))),
                ("B".into(), assessment(scores(6.0, 7.0, 5.0))),
            ]),
            confidence: 0.85,
            rationale: "A is correct and tested.".into(),
        },
    );
    log.push(
        EventKind::WinnerSelected(WinnerSelected {
            label: "A".into(),
            execution_id: exec(1, "A"),
            head_sha: "b".repeat(40),
            judgment_id: judgment_id(1),
            promotion: PromotionIntent::NotRequested,
        }),
        None,
    );
    // Round 2: a tie, so the judge hands it to the operator.
    run_round(&mut log, 2, 1, [Fate::Eligible, Fate::Eligible]);
    judge(
        &mut log,
        &paths,
        2,
        Judgment {
            schema_version: "1.0.0".into(),
            verdict: JudgmentVerdict::Tie,
            winner: None,
            ranking: vec!["B".into(), "A".into()],
            candidates: BTreeMap::from([
                ("A".into(), assessment(scores(8.0, 8.0, 8.0))),
                ("B".into(), assessment(scores(8.0, 8.0, 8.0))),
            ]),
            confidence: 0.5,
            rationale: "Equal.".into(),
        },
    );
    log.push(
        EventKind::RoundNeedsIntervention(RoundNeedsIntervention {
            reason: "tie".into(),
            source: InterventionSource::Judge,
        }),
        None,
    );
    record_outcome(
        &rec,
        &exp(1),
        &round(1),
        OutcomeKind::Verified,
        0.9,
        Some("shipped, no regressions".into()),
        outcome_at(),
    )
    .unwrap();

    let mut facts = InMemoryFacts::default();
    for n in 1..=3u64 {
        for (i, label) in ["A", "B"].iter().enumerate() {
            let status = if n == 3 && *label == "A" {
                ExecutionStatus::Failed {
                    failure: FailureKind::AgentExited { code: Some(1) },
                }
            } else {
                ExecutionStatus::Done
            };
            facts.insert(ExecutionFacts {
                execution_id: exec(n, label),
                status,
                session_id: None,
                transcript_ref: Some(format!("~/.claude/projects/r{n}/{label}.jsonl")),
                transcript_digest: Some(dg(&format!("transcript-{n}-{label}"))),
                started_at: "2026-10-02T12:00:01.000Z".into(),
                finished_at: Some("2026-10-02T12:05:00.500Z".into()),
                tokens: Tokens {
                    input: 1000 * n,
                    cache_write_5m: 0,
                    cache_write_1h: 0,
                    cache_read: 20_000,
                    output: 500,
                },
                // Odd micro-dollar amounts: the round's sum must be exact.
                cost_microusd: MicroUsd(100_001 * (i as i64 + 1) + n as i64),
                cost_source: CostSource::PriceTable {
                    date: "2026-09-28".into(),
                },
                skills: Vec::new(),
                routing: None,
            });
        }
    }
    (paths, rec, facts)
}

fn fixture_rows(state: &Path) -> Vec<ExportRow> {
    let (paths, _rec, facts) = build_fixture(state);
    export(&paths, &facts, LPV).unwrap()
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

// ─── EXP-03 ─────────────────────────────────────────────────────────────────

#[test]
fn exp_03_row_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let rows = fixture_rows(tmp.path());
    assert_eq!(rows.len(), 3);
    let ids: Vec<(ExperimentId, RoundId)> = rows
        .iter()
        .map(|r| (r.experiment_id.clone(), r.round_id.clone()))
        .collect();
    assert_eq!(
        ids,
        [(exp(1), round(1)), (exp(1), round(2)), (exp(2), round(3))]
    );

    for row in &rows {
        let v = serde_json::to_value(row).unwrap();
        assert_eq!(
            keys(&v),
            sorted(&[
                "schema",
                "api",
                "label_policy_version",
                "experiment_id",
                "round_id",
                "task_id",
                "state",
                "questions",
                "answers",
                "eligible_set",
                "planner_propensities",
                "teacher",
                "worker_runs",
                "winner",
                "component_quality",
                "cost_microusd",
                "latency_ms",
                "outcomes",
                "promotion",
            ])
        );
        assert_eq!(v["schema"], EXPORT_SCHEMA);
        assert_eq!(v["schema"], "mh.export/1.0.0");
        assert_eq!(v["api"], "systemone/v1");
        assert_eq!(v["label_policy_version"], LPV);
        assert_eq!(v["teacher"], json!({"id": "none", "probabilities": null}));
        assert_eq!(
            keys(&v["state"]),
            sorted(&[
                "task_features",
                "task_digest",
                "repo_digest",
                "environment_digest",
                "base_sha",
            ])
        );
        assert!(v["state"]["task_features"].is_object());
        assert_eq!(v["eligible_set"].as_array().unwrap().len(), 3);
        assert_eq!(v["planner_propensities"], json!({"A": 1.0, "B": 0.5}));
        assert_eq!(v["worker_runs"].as_array().unwrap().len(), 2);
        assert!(v["cost_microusd"].is_i64());
        assert!(v["latency_ms"].is_u64());
        assert!(v["outcomes"].is_array());
        assert!(v["promotion"].is_null());

        // One choice question and one score question per config.
        let q = &v["questions"];
        assert_eq!(
            keys(q),
            sorted(&[
                "best_worker",
                "quality:sonnet|claude|sonnet|high",
                "quality:codex-sol|codex|gpt-5-codex|-",
            ])
        );
        assert_eq!(q["best_worker"]["type"], "choice");
        for arm in &ARMS {
            assert_eq!(q[format!("quality:{}", arm.config_id)]["type"], "score");
        }
        assert_eq!(keys(&v["answers"]), keys(q));
    }

    // Round 1: the winner, its judged components and a summed projection.
    let r1 = serde_json::to_value(&rows[0]).unwrap();
    assert_eq!(
        r1["questions"]["best_worker"]["options"],
        json!(["codex-sol|codex|gpt-5-codex|-", "sonnet|claude|sonnet|high"])
    );
    assert_eq!(r1["winner"], "sonnet|claude|sonnet|high");
    assert_eq!(
        r1["answers"]["best_worker"],
        json!({"choice": "sonnet|claude|sonnet|high",
               "probabilities": {"sonnet|claude|sonnet|high": 1.0}, "confidence": null})
    );
    assert_eq!(
        r1["answers"]["quality:sonnet|claude|sonnet|high"]["probabilities"],
        json!({"score": 24.5})
    );
    assert_eq!(
        r1["component_quality"]["sonnet|claude|sonnet|high"],
        json!({"correctness": 9.0, "scope": 8.0, "tests": 7.5})
    );
    assert_eq!(
        r1["worker_runs"][0]["scores"]["judge_components"],
        json!({"correctness": 9.0, "scope": 8.0, "tests": 7.5})
    );
    // 100_002 + 200_003: summed exactly.
    assert_eq!(r1["cost_microusd"], 300_005);
    // winner.selected is the 16th event after round.created, at 1 s steps.
    assert_eq!(r1["latency_ms"], 16_000);
    assert_eq!(r1["state"]["task_features"]["round_index"], 0);

    // Round 2: a tie, judged but without a winner.
    let r2 = serde_json::to_value(&rows[1]).unwrap();
    assert!(r2["winner"].is_null());
    assert!(r2["answers"]["best_worker"].is_null());
    assert_eq!(r2["component_quality"].as_object().unwrap().len(), 2);

    // Round 3: rejected, never judged.
    let r3 = serde_json::to_value(&rows[2]).unwrap();
    assert!(r3["winner"].is_null());
    assert!(r3["answers"]["best_worker"].is_null());
    assert!(r3["questions"]["best_worker"].get("options").is_none());
    assert!(r3["answers"]["quality:sonnet|claude|sonnet|high"].is_null());
    assert_eq!(r3["component_quality"], json!({}));
    assert_eq!(r3["task_id"], "task-two");
}

#[test]
fn exp_03_other_label_policy_versions_are_not_exported() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, _rec, facts) = build_fixture(tmp.path());
    assert!(export(&paths, &facts, "lp-2").unwrap().is_empty());
}

// ─── EXP-04 ─────────────────────────────────────────────────────────────────

#[test]
fn exp_04_export_golden() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/export-1.0.0.jsonl");
    let tmp = tempfile::tempdir().unwrap();
    let actual = render_jsonl(&fixture_rows(tmp.path())).unwrap();

    if std::env::var("HORCH_BLESS").ok().as_deref() == Some("1") && !golden.exists() {
        std::fs::write(&golden, &actual).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&golden).unwrap_or_else(|e| {
        panic!(
            "{}: {e}. The export 1.0.0 golden is frozen; it is blessed once only",
            golden.display()
        )
    });
    assert_eq!(
        actual, want,
        "mh.export/1.0.0 changed. Never re-bless: a format change bumps the schema"
    );
}

#[test]
fn exp_04_regenerate_twice_identical() {
    // Two separate builds: different event ids, the same recorded facts.
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let (paths, _rec, facts) = build_fixture(a.path());
    let first = render_jsonl(&export(&paths, &facts, LPV).unwrap()).unwrap();
    let second = render_jsonl(&export(&paths, &facts, LPV).unwrap()).unwrap();
    let other = render_jsonl(&fixture_rows(b.path())).unwrap();
    assert_eq!(first, second);
    assert_eq!(first, other);
    assert_eq!(first.lines().count(), 3);

    // Every line is canonical JSON: sorted keys, no whitespace.
    for line in first.lines() {
        let v: Value = serde_json::from_str(line).unwrap();
        assert_eq!(horch_core::measure::digest::canonical_json(&v), line);
    }

    let rows = export(&paths, &facts, LPV).unwrap();
    let ts = t0() + Duration::days(1);
    let file = write_export(&paths, LPV, &rows, ts).unwrap();
    assert_eq!(
        file,
        paths
            .exports_dir(LPV)
            .unwrap()
            .join("20261003T120000.000Z.jsonl")
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), first);
    // The same export at the same time is accepted; the file is immutable.
    write_export(&paths, LPV, &rows, ts).unwrap();
    assert!(write_export(&paths, LPV, &rows[..1], ts).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // A label policy version is one plain path component.
    for bad in ["", "..", "a/b", "../x"] {
        assert!(write_export(&paths, bad, &[], ts).is_err(), "{bad:?}");
    }
    assert!(write_export(&paths, "lp-2", &rows, ts).is_err());
}

// ─── EXP-05 ─────────────────────────────────────────────────────────────────

/// `n` judged rows of one run each: row `i` runs arm `i % arms` on task
/// `i % tasks` and wins.
fn synthetic_rows(template: &ExportRow, n: usize, arms: usize, tasks: usize) -> Vec<ExportRow> {
    (0..n)
        .map(|i| {
            let mut row = template.clone();
            let arm = i % arms;
            row.task_id = TaskId::new(format!("task-{}", i % tasks)).unwrap();
            let mut run = row.worker_runs[0].clone();
            run.config.teammate = TeammateName::new(format!("t{arm}")).unwrap();
            run.config.harness = HarnessKind::Claude;
            run.config.model = ModelId::new(format!("m{arm}")).unwrap();
            run.config.effort = Some("high".into());
            run.config.config_id = format!("t{arm}|claude|m{arm}|high");
            row.winner = Some(run.config.config_id.clone());
            row.worker_runs = vec![run];
            row
        })
        .collect()
}

#[test]
fn exp_05_coverage_and_verdict() {
    let tmp = tempfile::tempdir().unwrap();
    let rows = fixture_rows(tmp.path());
    let t = ReadinessThresholds::builtin();
    assert_eq!(
        t,
        ReadinessThresholds {
            clef_judged_rounds: 50,
            clef_min_arms: 4,
            clef_runs_per_arm: 10,
            clef_distinct_tasks: 30,
            laya_judged_rounds: 500,
            laya_runs_per_arm: 50,
        }
    );

    // The fixture: 2 judged rounds of 3, 2 arms, 2 tasks.
    let report = readiness(&rows, &t);
    assert_eq!(report.judged_rounds, 2);
    assert_eq!(report.distinct_tasks, 2);
    let sonnet = &report.arms["sonnet|claude|sonnet|high"];
    assert_eq!(sonnet.runs, 3);
    assert_eq!(sonnet.terminal_runs, 3);
    assert_eq!(sonnet.judged_rounds, 2);
    assert_eq!(sonnet.wins, 1);
    assert_eq!(sonnet.distinct_tasks, 2);
    // Round 3's A failed its test gate: 5 of 6 gates passed.
    assert!((sonnet.gate_pass_rate - 5.0 / 6.0).abs() < 1e-12);
    let codex = &report.arms["codex-sol|codex|gpt-5-codex|-"];
    assert_eq!(codex.wins, 0);
    assert!((codex.gate_pass_rate - 5.0 / 6.0).abs() < 1e-12);
    assert_eq!(report.verdict, ReadinessVerdict::NotReady);

    // Below the Clef thresholds.
    let below = readiness(&synthetic_rows(&rows[0], 12, 2, 5), &t);
    assert_eq!(below.verdict, ReadinessVerdict::NotReady);
    assert_eq!(
        below.gaps,
        [
            "clef: judged rounds 12/50",
            "clef: arms with >= 10 runs 0/4",
            "clef: distinct tasks 5/30",
        ]
    );
    assert_eq!(below.arms["t0|claude|m0|high"].runs, 6);
    assert_eq!(below.arms["t0|claude|m0|high"].wins, 6);

    // Exactly at the Clef thresholds: 50 rounds, 4 arms of 12 or 13 runs, 30 tasks.
    let at = readiness(&synthetic_rows(&rows[0], 50, 4, 30), &t);
    assert_eq!(at.verdict, ReadinessVerdict::ClefReady);
    assert_eq!(
        at.gaps,
        [
            "laya: judged rounds 50/500",
            "laya: arms with >= 50 runs 0/4",
        ]
    );

    // One arm short of Clef.
    let short = readiness(&synthetic_rows(&rows[0], 50, 3, 30), &t);
    assert_eq!(short.verdict, ReadinessVerdict::NotReady);
    assert_eq!(short.gaps, ["clef: arms with >= 10 runs 3/4"]);

    // Above: Laya.
    let above = readiness(&synthetic_rows(&rows[0], 500, 4, 30), &t);
    assert_eq!(above.verdict, ReadinessVerdict::LayaReady);
    assert!(above.gaps.is_empty());

    // Rows without a judgment do not count as judged rounds.
    let mut unjudged = synthetic_rows(&rows[0], 50, 4, 30);
    unjudged[0].component_quality.clear();
    let report = readiness(&unjudged, &t);
    assert_eq!(report.judged_rounds, 49);
    assert_eq!(report.gaps, ["clef: judged rounds 49/50"]);

    // The caller's policy file overrides the built-in thresholds.
    let file = tmp.path().join("policy.json");
    std::fs::write(
        &file,
        r#"{"schema_version":"1.0.0","readiness":{"clef_judged_rounds":2,"clef_min_arms":2,
            "clef_runs_per_arm":3,"clef_distinct_tasks":2,"laya_judged_rounds":3,
            "laya_runs_per_arm":3}}"#,
    )
    .unwrap();
    let small = ReadinessThresholds::load(Some(&file)).unwrap();
    let report = readiness(&rows, &small);
    assert_eq!(report.verdict, ReadinessVerdict::ClefReady);
    assert_eq!(report.gaps, ["laya: judged rounds 2/3"]);
    assert_eq!(ReadinessThresholds::load(None).unwrap(), t);
    std::fs::write(
        &file,
        r#"{"schema_version":"1.0.0","readiness":{"bogus":1}}"#,
    )
    .unwrap();
    assert!(ReadinessThresholds::load(Some(&file)).is_err());
}

// ─── EXP-06 ─────────────────────────────────────────────────────────────────

#[test]
fn exp_06_outcome_recorded_and_exported() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, rec, facts) = build_fixture(tmp.path());

    let events = rec.read_all().unwrap().events;
    let outcome: Vec<&EventEnvelope> = events
        .iter()
        .filter(|e| e.kind == "outcome.recorded")
        .collect();
    assert_eq!(outcome.len(), 1);
    assert_eq!(
        outcome[0].idempotency_key,
        format!("outcome:{}:verified:2026-10-02T14:00:00.000Z", round(1))
    );
    assert_eq!(outcome[0].actor, Actor::Operator);
    assert_eq!(outcome[0].round_id, Some(round(1)));
    assert_eq!(
        outcome[0].payload,
        json!({"kind": "verified", "post_merge_score": 0.9,
               "note": "shipped, no regressions"})
    );

    // The same outcome again is a duplicate and writes nothing.
    let again = record_outcome(
        &rec,
        &exp(1),
        &round(1),
        OutcomeKind::Verified,
        0.9,
        Some("shipped, no regressions".into()),
        outcome_at(),
    )
    .unwrap();
    assert!(matches!(again, Appended::Duplicate(_)));

    // Scores outside 0..=1 are refused before anything is written.
    for bad in [f64::NAN, f64::INFINITY, -0.1, 1.5] {
        let err = record_outcome(
            &rec,
            &exp(1),
            &round(1),
            OutcomeKind::Regression,
            bad,
            None,
            outcome_at() + Duration::minutes(1),
        );
        assert!(err.is_err(), "{bad}");
    }
    assert_eq!(rec.read_all().unwrap().events.len(), events.len());

    // A later regression joins the first outcome in the row.
    record_outcome(
        &rec,
        &exp(1),
        &round(1),
        OutcomeKind::Regression,
        0.0,
        None,
        outcome_at() + Duration::days(1),
    )
    .unwrap();
    let rows = export(&paths, &facts, LPV).unwrap();
    assert_eq!(
        rows[0].outcomes,
        [
            OutcomeRecorded {
                kind: OutcomeKind::Verified,
                post_merge_score: 0.9,
                note: Some("shipped, no regressions".into()),
            },
            OutcomeRecorded {
                kind: OutcomeKind::Regression,
                post_merge_score: 0.0,
                note: None,
            },
        ]
    );
    assert!(rows[1].outcomes.is_empty());
    assert!(rows[2].outcomes.is_empty());
    assert_eq!(
        serde_json::to_value(&rows[0].outcomes[1]).unwrap(),
        json!({"kind": "regression", "post_merge_score": 0.0, "note": null})
    );
}

#[test]
fn exp_03_promotion_receipt_gives_dest_after() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, _rec, facts) = build_fixture(tmp.path());
    std::fs::write(
        paths.promotion(&round(1)).unwrap(),
        json!({"round_id": round(1).to_string(), "dest_before": "c".repeat(40),
               "dest_after": "d".repeat(40), "strategy": "fast_forward"})
        .to_string(),
    )
    .unwrap();
    let rows = export(&paths, &facts, LPV).unwrap();
    assert_eq!(
        serde_json::to_value(&rows[0].promotion).unwrap(),
        json!({"dest_after": "d".repeat(40)})
    );
    assert!(rows[1].promotion.is_none());

    std::fs::write(paths.promotion(&round(1)).unwrap(), "{}").unwrap();
    assert!(export(&paths, &facts, LPV).is_err());
}

#[test]
fn exp_04_round_id_with_a_path_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DatasetPaths::from_slug(tmp.path(), "forged");
    let rec = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();
    let mut log = Log {
        rec: &rec,
        exp: exp(1),
        round: None,
        at: t0(),
        n: 0,
    };
    log.push(
        EventKind::ExperimentCreated(experiment_created("task-one")),
        None,
    );
    log.push(
        EventKind::PreflightCompleted(PreflightCompleted {
            report: preflight_report(),
        }),
        None,
    );
    log.round = Some(RoundId::new("../../outside").unwrap());
    log.push(EventKind::RoundCreated(round_created(0)), None);
    for (label, (arm, p)) in ["A", "B"].iter().zip(ARMS.iter().zip([1.0, 0.5])) {
        log.push(
            EventKind::CandidatePlanned(planned(label, arm, p)),
            Some(exec(1, label)),
        );
    }
    for label in ["A", "B"] {
        log.push(
            EventKind::WorktreeCreated(WorktreeCreated {
                label: label.into(),
                path: format!("/wt/{label}").into(),
                branch: format!("mh/{label}"),
                base_sha: "a".repeat(40),
            }),
            Some(exec(1, label)),
        );
    }
    // RUNNING → NEEDS_INTERVENTION: the round is decided and exported.
    log.push(
        EventKind::RoundNeedsIntervention(RoundNeedsIntervention {
            reason: "operator".into(),
            source: InterventionSource::Operator,
        }),
        None,
    );
    let err = export(&paths, &InMemoryFacts::default(), LPV).unwrap_err();
    assert!(format!("{err:#}").contains("not a plain name"), "{err:#}");
}
