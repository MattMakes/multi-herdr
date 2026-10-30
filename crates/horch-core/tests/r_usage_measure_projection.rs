//! Characterisation of the projection fold's guards (issue #252). Mutated
//! lifecycles run through [`fold`]; the whole result is hashed against a
//! value recorded from the code before `apply_round` was split, and every
//! rejection text must appear at least once.

use chrono::{DateTime, TimeZone, Utc};
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
use horch_core::measure::projection::fold;
use horch_core::measure::testkit::SplitMix64;
use horch_core::measure::NumstatLine;
use horch_core::routing::decision::RoutingProvenance;
use horch_core::teacher::TeacherRef;
use serde_json::{json, Value};

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

// ─── mutated lifecycles ─────────────────────────────────────────────────────

/// Every (fates, judge failures, promote) script for 1 to 2 candidates.
fn scripts() -> Vec<Vec<EventEnvelope>> {
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
}

/// One random edit: drop, repeat, swap, a foreign event, or a changed field.
fn edit(rng: &mut SplitMix64, events: &mut Vec<EventEnvelope>, pool: &[EventEnvelope]) {
    let i = rng.below(events.len() as u64) as usize;
    match rng.below(9) {
        0 => {
            events.remove(i);
        }
        1 => {
            let e = events[i].clone();
            events.insert(i, e);
        }
        2 if i + 1 < events.len() => events.swap(i, i + 1),
        3 => {
            let e = pool[rng.below(pool.len() as u64) as usize].clone();
            events.insert(i, e);
        }
        4 => events[i].round_id = None,
        5 => {
            events[i].experiment_id =
                ExperimentId::new("0199a5b0-0000-7000-8000-0000000000ee").unwrap()
        }
        6 => {
            let mut e = events[i].clone();
            e.payload["labels"] = json!(["A", "A"]);
            e.kind = "round.created".into();
            events.insert(i, e);
        }
        _ => {
            let (key, value) = match rng.below(6) {
                0 => ("label", json!("Z")),
                1 => ("attempt", json!(rng.below(4))),
                2 => ("judgment_id", json!("j-2")),
                3 => ("final_outcome", json!("needs_intervention")),
                4 => ("source", json!("judge")),
                _ => ("label", json!("B")),
            };
            if events[i].payload.get(key).is_some() {
                events[i].payload[key] = value;
            }
        }
    }
}

/// A rejected round that an operator then promotes: it has no winner.
fn promote_without_winner() -> Vec<EventEnvelope> {
    let mut s = lifecycle(
        &[Fate {
            completes: true,
            eligible: false,
        }],
        0,
        false,
    );
    s.push(
        EventKind::OperatorPromote(OperatorPromote {
            target: "main".into(),
        }),
        None,
    );
    s.events
}

/// A judge that gives up before any judgment.
fn judge_intervention_without_judgment() -> Vec<EventEnvelope> {
    let ok = Fate {
        completes: true,
        eligible: true,
    };
    let mut s = lifecycle(&[ok], 0, false);
    s.events.retain(|e| {
        !matches!(
            e.kind.as_str(),
            "judge.completed" | "winner.selected" | "round.cleanup_started" | "round.completed"
        )
    });
    s.push(
        EventKind::RoundNeedsIntervention(RoundNeedsIntervention {
            reason: "tie".into(),
            source: InterventionSource::Judge,
        }),
        None,
    );
    s.events
}

/// Fold 6000 mutated lifecycles; the text of every projection, in order.
fn mutated_folds() -> Vec<String> {
    let scripts = scripts();
    let pool: Vec<EventEnvelope> = scripts.iter().flatten().cloned().collect();
    let mut rng = SplitMix64::new(0x252);
    let directed = [
        promote_without_winner(),
        judge_intervention_without_judgment(),
    ];
    let random = (0..6000).map(|_| {
        let mut events = scripts[rng.below(scripts.len() as u64) as usize].clone();
        for _ in 0..1 + rng.below(4) {
            edit(&mut rng, &mut events, &pool);
        }
        events
    });
    directed
        .into_iter()
        .chain(random)
        .map(|events| serde_json::to_string(&fold(&events)).unwrap())
        .collect()
}

/// A rejection text each guard of the fold can give.
const EXPECTED_REASONS: &[&str] = &[
    "already created",
    "appears twice",
    "unknown experiment",
    "unknown round",
    "belongs to experiment",
    "without a round_id",
    "unknown label",
    "repeated for",
    "ended",
    "was spawned",
    "was frozen",
    "after a judgment",
    "after attempt",
    "before a judgment",
    "cites judgment",
    "is not eligible",
    "is invalid in state",
    "for a round without a winner",
    "from the judge before a judgment",
    "cleanup started from",
    "for attempt",
    "scheduled",
];

#[test]
fn every_guard_gives_its_rejection_text() {
    let reasons: String = mutated_folds()
        .iter()
        .flat_map(|text| {
            let p: Value = serde_json::from_str(text).unwrap();
            p["anomalies"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a["reason"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n");
    for want in EXPECTED_REASONS {
        assert!(
            reasons.contains(want),
            "no rejection says {want:?}:\n{reasons}"
        );
    }
}

/// The folds of the mutated lifecycles equal those of the code before the
/// split, text for text.
#[test]
fn mutated_lifecycles_fold_as_they_did_before_the_split() {
    let all = mutated_folds().join("\n");
    let digest: Digest = sha256_bytes(all.as_bytes());
    assert_eq!(
        digest.to_string(),
        "sha256:0b94e973116bafb148748f4b4cbce0c4479725235f4b81399fc11f176322b575"
    );
}
