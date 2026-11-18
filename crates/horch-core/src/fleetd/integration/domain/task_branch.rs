//! The landing state machine of a task branch (ticket 25).
//!
//! `docs/specs/fleetd/design.md`, "Landing a task branch on its feature
//! branch" (the state table; F9), "A cancelled task does not block its
//! idea", "What a release does to a run in flight" (row Phase 4):
//! - [`next`]: where the branch goes from the state of its task. `building`
//!   follows the task to `verifying`, `verifying` to `ready` or back to
//!   `building`. A cancelled task's branch gets a rescue ref, then
//!   `abandoned`. The branch of an old claim attempt waits while its
//!   detached finish runs, and the old path lands it.
//! - [`owner`] and [`limit_s`]: the owner and the time limit of each state.

/// The landing state of a task branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandingState {
    Building,
    Verifying,
    Ready,
    InBundle,
    Landed,
    Conflicted,
    GateFailed,
    Abandoned,
}

impl LandingState {
    pub const ALL: [LandingState; 8] = [
        LandingState::Building,
        LandingState::Verifying,
        LandingState::Ready,
        LandingState::InBundle,
        LandingState::Landed,
        LandingState::Conflicted,
        LandingState::GateFailed,
        LandingState::Abandoned,
    ];

    /// The word of the state in `branches.landing_state`.
    pub fn as_str(self) -> &'static str {
        match self {
            LandingState::Building => "building",
            LandingState::Verifying => "verifying",
            LandingState::Ready => "ready",
            LandingState::InBundle => "in_bundle",
            LandingState::Landed => "landed",
            LandingState::Conflicted => "conflicted",
            LandingState::GateFailed => "gate_failed",
            LandingState::Abandoned => "abandoned",
        }
    }

    /// The state of the word `word`.
    pub fn parse(word: &str) -> Option<LandingState> {
        Self::ALL.into_iter().find(|s| s.as_str() == word)
    }

    /// `landed` and `abandoned` have no exit.
    pub fn is_terminal(self) -> bool {
        matches!(self, LandingState::Landed | LandingState::Abandoned)
    }
}

/// The owner of a branch in `state`: the column "Owner" of the table.
pub fn owner(state: LandingState) -> &'static str {
    match state {
        LandingState::Building => "worker",
        LandingState::Landed => "nobody",
        _ => "fleetd",
    }
}

/// The time limit keys of the landing states, in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// `landing_ready_limit_s`.
    pub ready_s: u64,
    /// `landing_repair_limit_s`: `conflicted` and `gate_failed`.
    pub repair_s: u64,
    /// The limit of the `bundle_build` job plus the limit of the
    /// `fast_gate` job plus `landing_job_margin_s`.
    pub in_bundle_s: u64,
}

/// The time limit of a branch in `state`. `building` and `verifying` have
/// the time limits of their task, which the stall ladder watches; a
/// terminal state has none.
pub fn limit_s(state: LandingState, limits: &Limits) -> Option<u64> {
    match state {
        LandingState::Ready => Some(limits.ready_s),
        LandingState::InBundle => Some(limits.in_bundle_s),
        LandingState::Conflicted | LandingState::GateFailed => Some(limits.repair_s),
        _ => None,
    }
}

/// The due time of a branch that enters `state` at the fleet clock
/// `entered`, or `None` for a state with no limit of its own.
pub fn due_at(state: LandingState, entered: i64, limits: &Limits) -> Option<i64> {
    let limit = i64::try_from(limit_s(state, limits)?).unwrap_or(i64::MAX);
    Some(entered.saturating_add(limit))
}

/// Whether a branch in `state`, due at `due_at`, is past its limit at the
/// fleet clock `clock`.
pub fn overdue(state: LandingState, due_at: i64, clock: i64, limits: &Limits) -> bool {
    limit_s(state, limits).is_some() && clock >= due_at
}

/// The state of the task of a branch, as the landing reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskFact {
    /// An attempt holds the task: `leased`, `running`, `stalled`,
    /// `blocked_on_user` or `waiting_quota`.
    Working,
    Verifying,
    Done,
    Cancelled,
    /// No attempt holds the task: `pending`, `ready`, `failed` or
    /// `escalated`.
    Between,
}

/// How the newest attempt of the task began (MAP.md of fleetd-p4, Q4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// `horch spawn` or the dispatch of fleetd bound it; fleetd lands it.
    Push,
    /// The old claim; the old finish lands it.
    Claim,
}

/// What [`next`] decides for 1 branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// The branch stays in its state.
    Stay,
    /// The branch moves through each state in order.
    Moves(Vec<LandingState>),
    /// A rescue ref of the branch first, then `abandoned`.
    Rescue,
}

/// Where a branch in `state` goes when its task is in `task`, its newest
/// attempt began by `via`, and `old_finish_runs` says whether a detached
/// finish of an old claim attempt runs.
///
/// - A detached old finish holds the branch: fleetd starts no landing of
///   that task until it ends (release row of Phase 4).
/// - F9: a cancelled task lands nothing. Each state that is not terminal
///   gets a rescue ref, then `abandoned`.
/// - The old path lands a claim attempt: its branch stays `building` and
///   goes to `landed` when its task is `done`, so the task lands 1 time.
/// - A push attempt: `building` follows the task to `verifying` and
///   `ready`, and `verifying` goes back to `building` when a new attempt
///   holds the task. The other states move by the bundle use cases.
pub fn next(state: LandingState, task: TaskFact, via: Via, old_finish_runs: bool) -> Next {
    if state.is_terminal() || old_finish_runs {
        return Next::Stay;
    }
    if task == TaskFact::Cancelled {
        return Next::Rescue;
    }
    use LandingState as S;
    let moves = match (via, state, task) {
        (Via::Claim, S::Building, TaskFact::Done) => vec![S::Landed],
        (Via::Claim, _, _) => vec![],
        (Via::Push, S::Building, TaskFact::Verifying) => vec![S::Verifying],
        (Via::Push, S::Building, TaskFact::Done) => vec![S::Verifying, S::Ready],
        (Via::Push, S::Verifying, TaskFact::Done) => vec![S::Ready],
        (Via::Push, S::Verifying, TaskFact::Working) => vec![S::Building],
        (Via::Push, _, _) => vec![],
    };
    if moves.is_empty() {
        Next::Stay
    } else {
        Next::Moves(moves)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LandingState as S;

    const LIMITS: Limits = Limits {
        ready_s: 1800,
        repair_s: 3600,
        in_bundle_s: 600 + 1200 + 600,
    };

    #[test]
    fn each_word_parses_back() {
        for s in S::ALL {
            assert_eq!(S::parse(s.as_str()), Some(s));
        }
        assert_eq!(S::parse("open"), None);
    }

    #[test]
    fn each_state_has_its_owner_and_limit() {
        assert_eq!(owner(S::Building), "worker");
        assert_eq!(owner(S::Verifying), "fleetd");
        assert_eq!(owner(S::Abandoned), "fleetd");
        assert_eq!(owner(S::Landed), "nobody");
        assert_eq!(due_at(S::Ready, 10, &LIMITS), Some(1810));
        assert_eq!(due_at(S::InBundle, 10, &LIMITS), Some(2410));
        assert_eq!(due_at(S::Conflicted, 10, &LIMITS), Some(3610));
        assert_eq!(due_at(S::GateFailed, 10, &LIMITS), Some(3610));
        for s in [S::Building, S::Verifying, S::Landed, S::Abandoned] {
            assert_eq!(due_at(s, 10, &LIMITS), None);
            assert!(!overdue(s, 0, i64::MAX, &LIMITS));
        }
        assert!(!overdue(S::Ready, 1810, 1809, &LIMITS));
        assert!(overdue(S::Ready, 1810, 1810, &LIMITS));
    }

    #[test]
    fn a_cancelled_task_rescues_each_state_that_is_not_terminal() {
        for s in S::ALL {
            let want = if s.is_terminal() {
                Next::Stay
            } else {
                Next::Rescue
            };
            assert_eq!(next(s, TaskFact::Cancelled, Via::Push, false), want);
            assert_eq!(next(s, TaskFact::Cancelled, Via::Claim, false), want);
        }
    }

    #[test]
    fn a_running_old_finish_holds_each_state() {
        for s in S::ALL {
            for t in [TaskFact::Done, TaskFact::Cancelled, TaskFact::Verifying] {
                assert_eq!(next(s, t, Via::Claim, true), Next::Stay);
            }
        }
    }

    #[test]
    fn only_a_claim_attempt_goes_from_building_to_landed() {
        assert_eq!(
            next(S::Building, TaskFact::Done, Via::Claim, false),
            Next::Moves(vec![S::Landed])
        );
        for s in S::ALL {
            for t in [
                TaskFact::Working,
                TaskFact::Verifying,
                TaskFact::Done,
                TaskFact::Between,
            ] {
                if let Next::Moves(m) = next(s, t, Via::Push, false) {
                    assert!(!m.contains(&S::Landed), "{s:?} {t:?} lands by push");
                }
            }
        }
    }

    #[test]
    fn a_push_branch_follows_its_task() {
        let n = |s, t| next(s, t, Via::Push, false);
        assert_eq!(n(S::Building, TaskFact::Working), Next::Stay);
        assert_eq!(
            n(S::Building, TaskFact::Verifying),
            Next::Moves(vec![S::Verifying])
        );
        assert_eq!(
            n(S::Building, TaskFact::Done),
            Next::Moves(vec![S::Verifying, S::Ready])
        );
        assert_eq!(n(S::Verifying, TaskFact::Done), Next::Moves(vec![S::Ready]));
        assert_eq!(
            n(S::Verifying, TaskFact::Working),
            Next::Moves(vec![S::Building])
        );
        assert_eq!(n(S::Verifying, TaskFact::Between), Next::Stay);
        for s in [S::Ready, S::InBundle, S::Conflicted, S::GateFailed] {
            assert_eq!(n(s, TaskFact::Done), Next::Stay);
        }
    }
}
