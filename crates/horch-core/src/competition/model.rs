//! Pure domain types of a competition: an experiment groups rounds, a round
//! runs anonymous candidates, and a judgment, a promotion and an outcome
//! follow. No I/O here (CMP-02).

use serde::{Deserialize, Serialize};

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
