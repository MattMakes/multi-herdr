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

/// The only utility rule: see `utility_winner`.
pub const UTILITY_RULE_V1: &str = "u1";

/// How a `tie` verdict is resolved (JDG-07). Serialized as
/// `{"mode":"disabled"}` or `{"mode":"utility","v":"u1"}`. The design's
/// §4.7 states the utility rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TieBreak {
    /// A tie needs the operator. The default.
    Disabled,
    /// `v` is the version of the utility rule. Only [`UTILITY_RULE_V1`]
    /// exists; any other `v` needs the operator.
    Utility { v: String },
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
/// `reject_all` is the judge's own finding that no candidate is acceptable,
/// so the round ends `Rejected{JudgeRejected}` with no operator step.
/// `abstain` says the evidence does not decide, so the operator decides.
/// The design's §4.7 table is this function.
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
            TieBreak::Utility { v } if v != UTILITY_RULE_V1 => {
                needs(&format!("the utility rule {v:?} is unknown"))
            }
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
            } else if !j.candidates.get(label).is_some_and(|a| a.acceptable) {
                // The judge contradicts itself: a winner must be acceptable.
                needs(&format!("the winner {label:?} is not marked acceptable"))
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

/// Utility rule `u1` (design §4.7): the tied labels are the eligible labels
/// the judge marked acceptable. The winner has the highest unweighted sum of
/// its component scores; equal sums go to the first label in label order.
/// The sum is computed here only and never stored (no composite score,
/// JDG-06). `confidence` does not gate a tie: the operator opted in.
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::evaluation::judgment::{CandidateAssessment, JUDGMENT_SCHEMA_VERSION};

    fn assessment(score: f64, acceptable: bool) -> CandidateAssessment {
        let scores = ["correctness", "tests", "scope", "maintainability", "risk"]
            .into_iter()
            .map(|c| (c.to_string(), score))
            .collect();
        CandidateAssessment {
            scores,
            acceptable,
            notes: String::new(),
        }
    }

    fn judgment(verdict: JudgmentVerdict, winner: Option<&str>) -> Judgment {
        let candidates: BTreeMap<String, CandidateAssessment> = [
            ("A".to_string(), assessment(6.0, true)),
            ("B".to_string(), assessment(8.0, false)),
        ]
        .into_iter()
        .collect();
        Judgment {
            schema_version: JUDGMENT_SCHEMA_VERSION.into(),
            verdict,
            winner: winner.map(str::to_string),
            ranking: vec!["B".into(), "A".into()],
            candidates,
            confidence: 0.9,
            rationale: String::new(),
        }
    }

    fn eligible() -> BTreeSet<String> {
        ["A", "B"].into_iter().map(String::from).collect()
    }

    /// The judge names a winner that it marked not acceptable: the answer
    /// contradicts itself, so the operator decides.
    #[test]
    fn a_winner_the_judge_marked_unacceptable_needs_the_operator() {
        let j = judgment(JudgmentVerdict::Winner, Some("B"));
        let outcome = decide_winner(Some(&j), &eligible(), &WinnerPolicy::default());
        assert!(
            matches!(outcome, WinnerOutcome::NeedsIntervention { ref reason } if reason.contains("acceptable")),
            "{outcome:?}"
        );
        let j = judgment(JudgmentVerdict::Winner, Some("A"));
        assert_eq!(
            decide_winner(Some(&j), &eligible(), &WinnerPolicy::default()),
            WinnerOutcome::Winner { label: "A".into() }
        );
    }

    /// Only the rule `u1` exists. A policy that names another rule never
    /// picks a winner with the `u1` rule.
    #[test]
    fn an_unknown_utility_rule_needs_the_operator() {
        let j = judgment(JudgmentVerdict::Tie, None);
        let known = WinnerPolicy {
            tie_break: TieBreak::Utility {
                v: UTILITY_RULE_V1.into(),
            },
            ..WinnerPolicy::default()
        };
        assert_eq!(
            decide_winner(Some(&j), &eligible(), &known),
            WinnerOutcome::Winner { label: "A".into() }
        );
        let unknown = WinnerPolicy {
            tie_break: TieBreak::Utility { v: "u2".into() },
            ..WinnerPolicy::default()
        };
        let outcome = decide_winner(Some(&j), &eligible(), &unknown);
        assert!(
            matches!(outcome, WinnerOutcome::NeedsIntervention { ref reason } if reason.contains("u2")),
            "{outcome:?}"
        );
    }
}
