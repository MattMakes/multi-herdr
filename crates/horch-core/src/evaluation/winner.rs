//! The winner policy (JDG-06, JDG-07): how a parsed judgment becomes a
//! winner, a rejection or a request for the operator.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::judgment::{Judgment, JudgmentVerdict};

/// Why a round ends without a winner. Recorded in the `winner.rejected`
/// event payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// No candidate passed validation, so the judge was skipped.
    NoEligible,
    /// The judge rejected every candidate, or chose one that is not eligible.
    JudgeRejected,
    BelowConfidence,
    Tie,
    /// The judged commits are no longer the candidates' commits (PRO-01).
    StaleJudgment,
    /// The integrated commit failed its gates (PRO-02).
    RevalidationFailed,
}

impl fmt::Display for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RejectReason::NoEligible => "no_eligible",
            RejectReason::JudgeRejected => "judge_rejected",
            RejectReason::BelowConfidence => "below_confidence",
            RejectReason::Tie => "tie",
            RejectReason::StaleJudgment => "stale_judgment",
            RejectReason::RevalidationFailed => "revalidation_failed",
        })
    }
}

/// The judge's policy for accepting a winner. It enters the judge policy
/// digest as canonical JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WinnerPolicy {
    /// A `winner` verdict below this confidence needs the operator.
    pub min_confidence: f64,
    pub tie_break: TieBreak,
}

impl Default for WinnerPolicy {
    fn default() -> Self {
        Self {
            min_confidence: 0.7,
            tie_break: TieBreak::Disabled,
        }
    }
}

/// SPEC-TODO(Spec B §11): the utility definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TieBreak {
    Disabled,
    /// `v` is the version of the utility rule.
    Utility {
        v: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum WinnerOutcome {
    Winner { label: String },
    NeedsIntervention { reason: String },
    Rejected { reason: RejectReason },
}

/// The design's winner table (JDG-06, JDG-07). Pure. `j` is `None` when no
/// attempt gave a valid judgment; `eligible` holds the labels that passed
/// validation. Component scores are read, never stored as one number.
///
/// SPEC-TODO(Spec B §11): confirm the Abstain and RejectAll mappings.
pub fn decide_winner(
    j: Option<&Judgment>,
    eligible: &BTreeSet<String>,
    policy: &WinnerPolicy,
) -> WinnerOutcome {
    if eligible.is_empty() {
        return WinnerOutcome::Rejected {
            reason: RejectReason::NoEligible,
        };
    }
    let Some(j) = j else {
        return needs("no valid judgment");
    };
    match j.verdict {
        JudgmentVerdict::RejectAll => WinnerOutcome::Rejected {
            reason: RejectReason::JudgeRejected,
        },
        JudgmentVerdict::Abstain => needs("the judge abstained"),
        JudgmentVerdict::Tie => match &policy.tie_break {
            TieBreak::Disabled => needs("the judge declared a tie and tie_break is disabled"),
            TieBreak::Utility { .. } => match utility_winner(j, eligible) {
                Some(label) => WinnerOutcome::Winner { label },
                None => needs("the judge declared a tie and no tied label is acceptable"),
            },
        },
        JudgmentVerdict::Winner => {
            let Some(label) = &j.winner else {
                return needs("the verdict is winner but no winner is named");
            };
            if !eligible.contains(label) {
                WinnerOutcome::Rejected {
                    reason: RejectReason::JudgeRejected,
                }
            } else if !matches!(
                j.confidence.partial_cmp(&policy.min_confidence),
                Some(Ordering::Greater | Ordering::Equal)
            ) {
                // A NaN on either side is not confident.
                needs(&format!(
                    "confidence {} is below {}",
                    j.confidence, policy.min_confidence
                ))
            } else {
                WinnerOutcome::Winner {
                    label: label.clone(),
                }
            }
        }
    }
}

fn needs(reason: &str) -> WinnerOutcome {
    WinnerOutcome::NeedsIntervention {
        reason: reason.to_string(),
    }
}

/// SPEC-TODO(Spec B §11 utility): the tied labels are the eligible labels
/// the judge marked acceptable. The winner has the highest sum of its
/// component scores; equal sums go to the first label in label order. The
/// sum is computed here only and never stored.
fn utility_winner(j: &Judgment, eligible: &BTreeSet<String>) -> Option<String> {
    let mut best: Option<(&String, f64)> = None;
    // BTreeMap order is label order, and only a strictly higher sum replaces
    // the current best.
    for (label, assessment) in &j.candidates {
        if !assessment.acceptable || !eligible.contains(label) {
            continue;
        }
        let sum: f64 = assessment.scores.values().sum();
        if best.is_none_or(|(_, b)| sum > b) {
            best = Some((label, sum));
        }
    }
    best.map(|(label, _)| label.clone())
}
