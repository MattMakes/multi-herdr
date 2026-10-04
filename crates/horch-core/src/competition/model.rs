//! Pure domain types of a competition: an experiment groups rounds, a round
//! runs anonymous candidates, and a judgment, a promotion and an outcome
//! follow. No I/O here (CMP-02).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::harness::HarnessKind;
use crate::ids::{ExecutionId, ExperimentId, ModelId, RoundId, TaskId, TeammateName};
use crate::measure::digest::Digest;
use crate::measure::event::{OutcomeKind, SlotKind};
use crate::usage::money::MicroUsd;

/// One task, run as one or more rounds under one configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Experiment {
    pub id: ExperimentId,
    pub task_id: TaskId,
    pub task_digest: Digest,
    pub config_digest: Digest,
    pub repo_digest: Digest,
    pub environment_digest: Digest,
    pub base_sha: String,
    /// Candidates per round.
    pub candidates: u32,
    pub strategy: String,
    pub budget: MicroUsd,
    /// `None`: no promotion (OD5).
    pub promote_to: Option<String>,
}

/// N candidates on one base SHA, judged together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Round {
    pub id: RoundId,
    pub experiment_id: ExperimentId,
    pub index: u32,
    pub base_sha: String,
    pub state: RoundState,
    pub seed: u64,
    pub label_policy_version: String,
    pub candidates: Vec<Candidate>,
}

/// A candidate's anonymous name in a round: `A`, `B`, … `Z`, then `AA`, …
/// The judge sees only this, never the teammate, model or cost.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CandidateLabel(String);

/// A label that is not 1 or more ASCII capital letters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadLabel(pub String);

impl fmt::Display for BadLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "candidate label '{}' must be ASCII capital letters",
            self.0
        )
    }
}

impl std::error::Error for BadLabel {}

impl CandidateLabel {
    pub fn new(value: impl Into<String>) -> Result<Self, BadLabel> {
        let value = value.into();
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(BadLabel(value));
        }
        Ok(Self(value))
    }

    /// The `index`-th label in spreadsheet order: 0 → `A`, 25 → `Z`, 26 → `AA`.
    pub fn from_index(index: usize) -> Self {
        let mut n = index + 1;
        let mut out = Vec::new();
        while n > 0 {
            n -= 1;
            out.push(b'A' + (n % 26) as u8);
            n /= 26;
        }
        out.reverse();
        Self(String::from_utf8(out).expect("ASCII"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CandidateLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for CandidateLabel {
    type Error = BadLabel;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CandidateLabel> for String {
    fn from(label: CandidateLabel) -> String {
        label.0
    }
}

/// A planned execution plus its anonymous label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub label: CandidateLabel,
    pub execution_id: ExecutionId,
    pub config_id: String,
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: ModelId,
    pub effort: Option<String>,
    pub slot: SlotKind,
    pub propensity: f64,
}

/// An opt-in promotion of a round's winner (OD5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Promotion {
    pub round_id: RoundId,
    pub label: CandidateLabel,
    pub target: String,
    pub dest_before: String,
    /// `None` until the ref moved.
    pub dest_after: Option<String>,
    /// `None` until the receipt is durable.
    pub receipt_digest: Option<Digest>,
}

/// What happened to a promoted change later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub round_id: RoundId,
    pub kind: OutcomeKind,
    pub post_merge_score: f64,
    pub note: Option<String>,
}

/// Where a round is (dataset design §5). The first four states belong to the
/// experiment before its first round: CREATED, PREFLIGHT, ABORTED, PLANNED.
///
/// SPEC-TODO(Spec B round states): the master plan names only the states
/// from JUDGING_BACKGROUND on; the earlier states follow the B2/B3 flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoundState {
    Created,
    Preflight,
    Aborted,
    Planned,
    Provisioning,
    Running,
    Validating,
    JudgingBackground,
    Decided,
    Revalidating,
    Promoting,
    Promoted,
    NeedsIntervention,
    Rejected,
    Cleanup,
    Complete,
}

impl RoundState {
    /// The spelling in design §5, such as `JUDGING_BACKGROUND`.
    pub fn as_str(self) -> &'static str {
        match self {
            RoundState::Created => "CREATED",
            RoundState::Preflight => "PREFLIGHT",
            RoundState::Aborted => "ABORTED",
            RoundState::Planned => "PLANNED",
            RoundState::Provisioning => "PROVISIONING",
            RoundState::Running => "RUNNING",
            RoundState::Validating => "VALIDATING",
            RoundState::JudgingBackground => "JUDGING_BACKGROUND",
            RoundState::Decided => "DECIDED",
            RoundState::Revalidating => "REVALIDATING",
            RoundState::Promoting => "PROMOTING",
            RoundState::Promoted => "PROMOTED",
            RoundState::NeedsIntervention => "NEEDS_INTERVENTION",
            RoundState::Rejected => "REJECTED",
            RoundState::Cleanup => "CLEANUP",
            RoundState::Complete => "COMPLETE",
        }
    }
}

impl std::fmt::Display for RoundState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
