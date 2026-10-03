//! The round state machine (dataset design §5, CMP-03).
//!
//! [`TABLE`] holds every allowed transition, for the default path and the
//! promotion path (OD5). [`transition`] is the only place a round's state
//! changes; `measure::projection` maps each event to a [`RoundEvent`] and
//! records an anomaly when the table has no row for it.
//!
//! A [`RoundEvent`] is an event kind plus the condition design §5 attaches
//! to it, such as "the last candidate became terminal". Its name is the
//! event kind, with a `:<condition>` suffix when the condition picks the
//! target state.
//!
//! SPEC-TODO(Spec B round states): the states before JUDGING_BACKGROUND
//! follow the B2/B3 flow, not Spec B text.

use std::fmt;

use crate::competition::model::RoundState;
use crate::measure::event::InterventionSource;

/// A round-moving event, with the condition that picks its target state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundEvent {
    ExperimentCreated,
    /// `preflight.completed` with every check passed.
    PreflightPassed,
    /// `preflight.completed` with a failed check: `experiment.aborted` follows.
    PreflightFailed,
    ExperimentAborted,
    RoundCreated,
    /// `last`: every other candidate is already planned.
    CandidatePlanned {
        last: bool,
    },
    /// `last`: every other candidate already has its worktree.
    WorktreeCreated {
        last: bool,
    },
    CandidateSpawned,
    /// `candidate.completed` or `candidate.failed`. `last`: every other
    /// candidate is already terminal.
    CandidateEnded {
        last: bool,
    },
    CandidateFrozen,
    /// `last`: every other candidate is validated. `any_eligible`: at least
    /// 1 candidate, this one included, passed every gate.
    ValidationCompleted {
        last: bool,
        any_eligible: bool,
    },
    JudgeScheduled,
    JudgeStarted,
    JudgeCompleted,
    /// `last_attempt`: the attempt was the final one.
    JudgeFailed {
        last_attempt: bool,
    },
    WinnerSelected,
    /// A promotion is requested for a DECIDED winner: `winner.selected` with
    /// `promotion: requested`, or a re-entry by `promote <round>`.
    PromotionRequested,
    WinnerRejected,
    PromotionStarted,
    PromotionConflicted,
    PromotionCompleted,
    PromotionRolledBack,
    NeedsIntervention(InterventionSource),
    /// The operator's `promote <round>`: back to DECIDED (only for a round
    /// with a winner; the caller checks that).
    OperatorPromote,
    /// `round.cleanup_started`. From NEEDS_INTERVENTION only on the
    /// operator's `cleanup` command: the coordinator never emits it there.
    CleanupStarted,
    RoundCompleted,
}

impl RoundEvent {
    /// The key of this event in [`TABLE`].
    pub fn name(&self) -> &'static str {
        match self {
            RoundEvent::ExperimentCreated => "experiment.created",
            RoundEvent::PreflightPassed => "preflight.completed:passed",
            RoundEvent::PreflightFailed => "preflight.completed:failed",
            RoundEvent::ExperimentAborted => "experiment.aborted",
            RoundEvent::RoundCreated => "round.created",
            RoundEvent::CandidatePlanned { last: false } => "candidate.planned",
            RoundEvent::CandidatePlanned { last: true } => "candidate.planned:last",
            RoundEvent::WorktreeCreated { last: false } => "worktree.created",
            RoundEvent::WorktreeCreated { last: true } => "worktree.created:last",
            RoundEvent::CandidateSpawned => "candidate.spawned",
            RoundEvent::CandidateEnded { last: false } => "candidate.ended",
            RoundEvent::CandidateEnded { last: true } => "candidate.ended:last",
            RoundEvent::CandidateFrozen => "candidate.frozen",
            RoundEvent::ValidationCompleted { last: false, .. } => "validation.completed",
            RoundEvent::ValidationCompleted {
                last: true,
                any_eligible: true,
            } => "validation.completed:last",
            RoundEvent::ValidationCompleted {
                last: true,
                any_eligible: false,
            } => "validation.completed:none_eligible",
            RoundEvent::JudgeScheduled => "judge.scheduled",
            RoundEvent::JudgeStarted => "judge.started",
            RoundEvent::JudgeCompleted => "judge.completed",
            RoundEvent::JudgeFailed {
                last_attempt: false,
            } => "judge.failed",
            RoundEvent::JudgeFailed { last_attempt: true } => "judge.failed:last_attempt",
            RoundEvent::WinnerSelected => "winner.selected",
            RoundEvent::PromotionRequested => "promotion.requested",
            RoundEvent::WinnerRejected => "winner.rejected",
            RoundEvent::PromotionStarted => "promotion.started",
            RoundEvent::PromotionConflicted => "promotion.conflicted",
            RoundEvent::PromotionCompleted => "promotion.completed",
            RoundEvent::PromotionRolledBack => "promotion.rolled_back",
            RoundEvent::NeedsIntervention(InterventionSource::Judge) => {
                "round.needs_intervention:judge"
            }
            RoundEvent::NeedsIntervention(InterventionSource::Promotion) => {
                "round.needs_intervention:promotion"
            }
            RoundEvent::NeedsIntervention(InterventionSource::Operator) => {
                "round.needs_intervention:operator"
            }
            RoundEvent::OperatorPromote => "operator.promote",
            RoundEvent::CleanupStarted => "round.cleanup_started",
            RoundEvent::RoundCompleted => "round.completed",
        }
    }
}

impl fmt::Display for RoundEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// An event the table does not allow in a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: RoundState,
    pub event: RoundEvent,
}

impl fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is invalid in state {}", self.event, self.from)
    }
}

impl std::error::Error for InvalidTransition {}

use RoundState::*;

/// Every allowed transition: (from, [`RoundEvent::name`], to). Every other
/// pair is invalid.
pub const TABLE: &[(RoundState, &str, RoundState)] = &[
    // The experiment, before its first round.
    (Created, "experiment.created", Preflight),
    (Preflight, "preflight.completed:passed", Planned),
    (Preflight, "preflight.completed:failed", Preflight),
    (Created, "experiment.aborted", Aborted),
    (Preflight, "experiment.aborted", Aborted),
    // A round is born PLANNED.
    (Planned, "round.created", Planned),
    (Planned, "candidate.planned", Planned),
    (Planned, "candidate.planned:last", Provisioning),
    (Provisioning, "worktree.created", Provisioning),
    (Provisioning, "worktree.created:last", Running),
    (Running, "candidate.spawned", Running),
    (Running, "candidate.ended", Running),
    (Running, "candidate.ended:last", Validating),
    (Validating, "candidate.frozen", Validating),
    (Validating, "validation.completed", Validating),
    (Validating, "validation.completed:last", JudgingBackground),
    // 0 eligible: the judge is skipped, `winner.rejected{no_eligible}` follows.
    (Validating, "validation.completed:none_eligible", Validating),
    (Validating, "winner.rejected", Rejected),
    (JudgingBackground, "judge.scheduled", JudgingBackground),
    (JudgingBackground, "judge.started", JudgingBackground),
    (JudgingBackground, "judge.completed", JudgingBackground),
    (JudgingBackground, "judge.failed", JudgingBackground),
    (
        JudgingBackground,
        "judge.failed:last_attempt",
        NeedsIntervention,
    ),
    (JudgingBackground, "winner.selected", Decided),
    (JudgingBackground, "winner.rejected", Rejected),
    (
        JudgingBackground,
        "round.needs_intervention:judge",
        NeedsIntervention,
    ),
    // Default path (OD5): DECIDED → CLEANUP → COMPLETE.
    (Decided, "round.cleanup_started", Cleanup),
    (Rejected, "round.cleanup_started", Cleanup),
    (Cleanup, "round.completed", Complete),
    // Promotion path: DECIDED → REVALIDATING → PROMOTING → PROMOTED.
    (Decided, "promotion.requested", Revalidating),
    (Revalidating, "winner.rejected", Rejected),
    (Revalidating, "promotion.conflicted", NeedsIntervention),
    (
        Revalidating,
        "round.needs_intervention:promotion",
        NeedsIntervention,
    ),
    (Revalidating, "promotion.started", Promoting),
    (Promoting, "promotion.completed", Promoted),
    (Promoting, "promotion.conflicted", NeedsIntervention),
    (
        Promoting,
        "round.needs_intervention:promotion",
        NeedsIntervention,
    ),
    (Promoted, "round.cleanup_started", Cleanup),
    (Promoted, "promotion.rolled_back", Promoted),
    (Complete, "promotion.rolled_back", Complete),
    // The operator stops a live round.
    (
        Running,
        "round.needs_intervention:operator",
        NeedsIntervention,
    ),
    (
        Validating,
        "round.needs_intervention:operator",
        NeedsIntervention,
    ),
    (
        JudgingBackground,
        "round.needs_intervention:operator",
        NeedsIntervention,
    ),
    (
        Decided,
        "round.needs_intervention:operator",
        NeedsIntervention,
    ),
    (
        Revalidating,
        "round.needs_intervention:operator",
        NeedsIntervention,
    ),
    (
        Promoting,
        "round.needs_intervention:operator",
        NeedsIntervention,
    ),
    // NEEDS_INTERVENTION keeps every worktree. Only the operator moves it on:
    // `promote <round>` (a round with a winner) or `cleanup`.
    (NeedsIntervention, "operator.promote", Decided),
    (NeedsIntervention, "round.cleanup_started", Cleanup),
    // SPEC-TODO(Spec B §promote): `promote <round>` "later re-enters at
    // DECIDED"; a default round is COMPLETE by then, its branches kept.
    (Complete, "operator.promote", Decided),
];

/// The state `event` moves a round in `from` to.
pub fn transition(from: RoundState, event: &RoundEvent) -> Result<RoundState, InvalidTransition> {
    let name = event.name();
    TABLE
        .iter()
        .find(|(f, e, _)| *f == from && *e == name)
        .map(|(_, _, to)| *to)
        .ok_or(InvalidTransition {
            from,
            event: *event,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_no_duplicate_keys() {
        for (i, (f, e, _)) in TABLE.iter().enumerate() {
            assert!(
                !TABLE[..i].iter().any(|(f2, e2, _)| f2 == f && e2 == e),
                "{f} {e} appears twice"
            );
        }
    }
}
