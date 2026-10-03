//! Operator outcomes (EXP-06): what happened to a round's change after the
//! merge. An outcome is an `outcome.recorded` event; the fold attaches it to
//! its round and [`export`](super::export::export) carries it in the row.

use std::fmt;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::ids::{ExperimentId, RoundId};
use crate::measure::event::{format_occurred_at, Actor, EventKind, OutcomeKind, OutcomeRecorded};
use crate::measure::recorder::{Appended, NewEvent, Recorder};

/// Why an outcome is refused before anything is recorded.
#[derive(Debug, Clone, PartialEq)]
pub enum OutcomeError {
    /// `post_merge_score` is not a finite number in 0.0..=1.0.
    ScoreOutOfRange(f64),
}

impl fmt::Display for OutcomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutcomeError::ScoreOutOfRange(score) => {
                write!(
                    f,
                    "post_merge_score {score} is not a finite number in 0..=1"
                )
            }
        }
    }
}

impl std::error::Error for OutcomeError {}

/// The serde spelling of `kind`, as the idempotency key uses it.
fn kind_name(kind: OutcomeKind) -> &'static str {
    match kind {
        OutcomeKind::Regression => "regression",
        OutcomeKind::Revert => "revert",
        OutcomeKind::Verified => "verified",
    }
}

/// Append `outcome.recorded` for `round`, with the idempotency key
/// `outcome:<round>:<kind>:<at>`. The fold applies it only to a round in
/// DECIDED, PROMOTED, CLEANUP or COMPLETE.
pub fn record_outcome(
    recorder: &dyn Recorder,
    experiment: &ExperimentId,
    round: &RoundId,
    kind: OutcomeKind,
    post_merge_score: f64,
    note: Option<String>,
    at: DateTime<Utc>,
) -> Result<Appended> {
    if !post_merge_score.is_finite() || !(0.0..=1.0).contains(&post_merge_score) {
        return Err(OutcomeError::ScoreOutOfRange(post_merge_score).into());
    }
    recorder.append(NewEvent {
        kind: EventKind::OutcomeRecorded(OutcomeRecorded {
            kind,
            post_merge_score,
            note,
        }),
        actor: Actor::Operator,
        experiment_id: experiment.clone(),
        round_id: Some(round.clone()),
        execution_id: None,
        idempotency_key: format!(
            "outcome:{round}:{}:{}",
            kind_name(kind),
            format_occurred_at(at)
        ),
        occurred_at: at,
    })
}
