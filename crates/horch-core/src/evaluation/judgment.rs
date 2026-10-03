//! The judge's answer (JDG-05, JDG-06) and the record the coordinator keeps.
//!
//! A [`Judgment`] only ever comes out of
//! [`parse_judgment`](super::parser::parse_judgment), which checks it against
//! the round's labels and the rubric. Component scores stay per component;
//! nothing here folds them into one number.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{ExecutionId, JudgmentId, RoundId};
use crate::measure::digest::Digest;

/// The version of the [`Judgment`] shape and of
/// `assets/judge/judgment-schema-1.0.0.json`.
pub const JUDGMENT_SCHEMA_VERSION: &str = "1.0.0";

/// The field names of [`Judgment`], in declaration order.
pub const JUDGMENT_FIELDS: &[&str] = &[
    "schema_version",
    "verdict",
    "winner",
    "ranking",
    "candidates",
    "confidence",
    "rationale",
];

/// The fields of [`Judgment`] that may be left out: `winner` is required
/// only when the verdict is `winner`, which the parser checks.
pub const JUDGMENT_OPTIONAL_FIELDS: &[&str] = &["winner"];

/// The field names of [`CandidateAssessment`], in declaration order.
pub const ASSESSMENT_FIELDS: &[&str] = &["scores", "acceptable", "notes"];

/// The serde spellings of [`JudgmentVerdict`].
pub const VERDICTS: &[&str] = &["winner", "tie", "abstain", "reject_all"];

/// SPEC-TODO(Spec B §11): the Judgment schema verbatim. This shape is provisional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Judgment {
    /// [`JUDGMENT_SCHEMA_VERSION`].
    pub schema_version: String,
    pub verdict: JudgmentVerdict,
    /// A label; required when the verdict is `winner`, absent otherwise.
    pub winner: Option<String>,
    /// Every label exactly once, best first.
    pub ranking: Vec<String>,
    /// One assessment per label.
    pub candidates: BTreeMap<String, CandidateAssessment>,
    /// Finite, 0.0..=1.0.
    pub confidence: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgmentVerdict {
    Winner,
    Tie,
    Abstain,
    RejectAll,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateAssessment {
    /// Rubric component → score. Kept as given, never collapsed (JDG-06).
    pub scores: BTreeMap<String, f64>,
    pub acceptable: bool,
    pub notes: String,
}

/// The record the coordinator writes to `judgements/<round>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgmentRecord {
    pub judgment_id: JudgmentId,
    pub round_id: RoundId,
    pub attempt: u32,
    pub input_digest: Digest,
    pub judge_policy_digest: Digest,
    pub execution_id: ExecutionId,
    pub judgment: Judgment,
}
