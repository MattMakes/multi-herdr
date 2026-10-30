//! Projection values for the tests of the `status` and `watch` views.

use horch_core::competition::model::RoundState;
use horch_core::ids::{ExperimentId, RoundId};
use horch_core::measure::digest::sha256_bytes;
use horch_core::measure::projection::{CandidateView, ExperimentView, JudgeView, PromotionView};
use horch_core::measure::projection::{Projection, RoundView};
use horch_core::teacher::TeacherRef;
use serde_json::{json, Value};

pub(crate) const EXP_ID: &str = "0199a5b0-0000-7000-8000-000000000e01";

pub(crate) fn exp() -> ExperimentId {
    ExperimentId::new(EXP_ID).unwrap()
}

/// An experiment in `state` with the rounds `rounds`.
pub(crate) fn experiment(state: RoundState, rounds: Vec<RoundId>) -> ExperimentView {
    serde_json::from_value(json!({
        "state": state,
        "created": {
            "task_id": "task-1",
            "task_digest": sha256_bytes(b"task"),
            "config_digest": sha256_bytes(b"config"),
            "base_sha": "0123456789abcdef0123456789abcdef01234567",
            "repo_digest": sha256_bytes(b"repo"),
            "environment_digest": sha256_bytes(b"env"),
            "candidates": 2,
            "strategy": "diverse",
            "budget_usd_micro": 5_000_000,
            "promote_to": null,
        },
        "preflight": null,
        "aborted": null,
        "rounds": rounds,
    }))
    .unwrap()
}

/// A round in `state` that holds the candidates `candidates`.
pub(crate) fn round_view(state: RoundState, candidates: Vec<(&str, CandidateView)>) -> RoundView {
    RoundView {
        experiment_id: exp(),
        state,
        created: serde_json::from_value(json!({
            "index": 0,
            "base_sha": "a",
            "labels": [],
            "eligible_set": [],
            "propensities": {},
            "teacher": TeacherRef::none(),
            "seed": 1,
            "label_policy_version": "slot-order-1",
        }))
        .unwrap(),
        candidates: candidates
            .into_iter()
            .map(|(l, c)| (l.to_string(), c))
            .collect(),
        judge: JudgeView::default(),
        winner: None,
        rejected: None,
        promotion: PromotionView::default(),
        needs_intervention: None,
        cleaned_from: None,
        final_outcome: None,
        cleanup_failures: Vec::new(),
        outcomes: Vec::new(),
    }
}

/// The experiment [`exp`] in `state`, with 1 round per entry of `rounds`
/// (in the state given). Gives the projection.
pub(crate) fn projection(state: RoundState, rounds: &[RoundState]) -> Projection {
    let mut p = Projection::default();
    let mut ids = Vec::new();
    for (i, r) in rounds.iter().enumerate() {
        let id = RoundId::new(format!("0199a5b0-0000-7000-8000-000000000a0{i}")).unwrap();
        p.rounds.insert(id.clone(), round_view(*r, Vec::new()));
        ids.push(id);
    }
    p.experiments.insert(exp(), experiment(state, ids));
    p
}

/// A candidate with the parts `parts` set, each as a JSON object.
/// `parts` keys: `planned`, `worktree`, `spawned`, `completed`, `failed`,
/// `frozen`, `validation`.
pub(crate) fn candidate(parts: &[&str]) -> CandidateView {
    let all: Value = json!({
        "planned": {"label": "A", "teammate": "sonnet", "harness": "claude", "model": "sonnet",
                    "effort": null, "slot": "diversity", "propensity": 1.0, "config_id": "c"},
        "worktree": {"label": "A", "path": "/w", "branch": "b", "base_sha": "x"},
        "spawned": {"label": "A", "pane": "p1",
                    "routing": {"requested": "sonnet", "resolved": "sonnet", "fallback_index": null,
                                "pool": "claude", "pool_state": "ok", "reason": null,
                                "mode": "auto"}},
        "completed": {"label": "A", "exit_code": 0},
        "failed": {"label": "A", "failure": {"kind": "agent_exited", "code": 3}},
        "frozen": {"label": "A", "head_sha": "h", "numstat": [],
                   "diff_digest": sha256_bytes(b"diff")},
        "validation": {"validation_id": "v", "label": "A", "head_sha": "h", "gates": [],
                       "mechanical_score": 0.5, "eligible": true},
    });
    let mut view = serde_json::Map::new();
    for part in parts {
        view.insert(part.to_string(), all[part].clone());
    }
    serde_json::from_value(Value::Object(view)).unwrap()
}
