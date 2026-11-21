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

    pub fn parse(word: &str) -> Option<LandingState> {
        Self::ALL.into_iter().find(|s| s.as_str() == word)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, LandingState::Landed | LandingState::Abandoned)
    }
}

pub fn owner(state: LandingState) -> &'static str {
    match state {
        LandingState::Building => "worker",
        LandingState::Landed => "nobody",
        _ => "fleetd",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub ready_s: u64,
    pub repair_s: u64,
    pub in_bundle_s: u64,
}

pub fn limit_s(state: LandingState, limits: &Limits) -> Option<u64> {
    match state {
        LandingState::Ready => Some(limits.ready_s),
        LandingState::InBundle => Some(limits.in_bundle_s),
        LandingState::Conflicted | LandingState::GateFailed => Some(limits.repair_s),
        _ => None,
    }
}

pub fn due_at(state: LandingState, entered: i64, limits: &Limits) -> Option<i64> {
    let limit = i64::try_from(limit_s(state, limits)?).unwrap_or(i64::MAX);
    Some(entered.saturating_add(limit))
}

pub fn overdue(state: LandingState, due_at: i64, clock: i64, limits: &Limits) -> bool {
    limit_s(state, limits).is_some() && clock >= due_at
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskFact {
    Working,
    Verifying,
    Done,
    Cancelled,
    Between,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    Stay,
    Moves(Vec<LandingState>),
    Rescue,
}

pub fn next(state: LandingState, task: TaskFact) -> Next {
    if state.is_terminal() {
        return Next::Stay;
    }
    if task == TaskFact::Cancelled {
        return Next::Rescue;
    }
    use LandingState as S;
    let moves = match (state, task) {
        (S::Building, TaskFact::Verifying) => vec![S::Verifying],
        (S::Building, TaskFact::Done) => vec![S::Verifying, S::Ready],
        (S::Verifying, TaskFact::Done) => vec![S::Ready],
        (S::Verifying, TaskFact::Working) => vec![S::Building],
        _ => vec![],
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
            assert_eq!(next(s, TaskFact::Cancelled), want);
        }
    }

    #[test]
    fn no_branch_goes_from_building_to_landed() {
        for s in S::ALL {
            for t in [
                TaskFact::Working,
                TaskFact::Verifying,
                TaskFact::Done,
                TaskFact::Between,
            ] {
                if let Next::Moves(m) = next(s, t) {
                    assert!(!m.contains(&S::Landed), "{s:?} {t:?} lands");
                }
            }
        }
    }

    #[test]
    fn a_push_branch_follows_its_task() {
        let n = next;
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
