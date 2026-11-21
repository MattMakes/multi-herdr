use crate::fleetd::integration::domain::task_branch::LandingState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundleState {
    Building,
    Gating,
    Passed,
    Landed,
    Failed,
    Dropped,
}

impl BundleState {
    pub const ALL: [BundleState; 6] = [
        BundleState::Building,
        BundleState::Gating,
        BundleState::Passed,
        BundleState::Landed,
        BundleState::Failed,
        BundleState::Dropped,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            BundleState::Building => "building",
            BundleState::Gating => "gating",
            BundleState::Passed => "passed",
            BundleState::Landed => "landed",
            BundleState::Failed => "failed",
            BundleState::Dropped => "dropped",
        }
    }

    pub fn parse(word: &str) -> Option<BundleState> {
        Self::ALL.into_iter().find(|s| s.as_str() == word)
    }

    pub fn is_open(self) -> bool {
        matches!(
            self,
            BundleState::Building | BundleState::Gating | BundleState::Passed
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub building_s: u64,
    pub gating_s: u64,
}

pub fn limit_s(state: BundleState, limits: &Limits) -> Option<u64> {
    match state {
        BundleState::Building => Some(limits.building_s),
        BundleState::Gating => Some(limits.gating_s),
        _ => None,
    }
}

pub fn due_at(state: BundleState, entered: i64, limits: &Limits) -> Option<i64> {
    let limit = i64::try_from(limit_s(state, limits)?).unwrap_or(i64::MAX);
    Some(entered.saturating_add(limit))
}

pub fn overdue(state: BundleState, due_at: i64, clock: i64, limits: &Limits) -> bool {
    limit_s(state, limits).is_some() && clock >= due_at
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildFact {
    Pending,
    Abandoned,
    Built {
        candidate_ref: String,
        candidate_tip: String,
    },
    Conflict {
        member: String,
    },
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropCause {
    TipMoved,
    BuildFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Stay,
    Rebuild,
    Gate {
        candidate_ref: String,
        candidate_tip: String,
    },
    Conflict {
        member: String,
    },
    Drop(DropCause),
}

pub fn building(tip_moved: bool, build: BuildFact) -> Step {
    if tip_moved {
        return Step::Drop(DropCause::TipMoved);
    }
    match build {
        BuildFact::Pending => Step::Stay,
        BuildFact::Abandoned => Step::Rebuild,
        BuildFact::Built {
            candidate_ref,
            candidate_tip,
        } => Step::Gate {
            candidate_ref,
            candidate_tip,
        },
        BuildFact::Conflict { member } => Step::Conflict { member },
        BuildFact::Failed => Step::Drop(DropCause::BuildFailed),
    }
}

pub fn gating(tip_moved: bool) -> Step {
    if tip_moved {
        Step::Drop(DropCause::TipMoved)
    } else {
        Step::Stay
    }
}

pub fn member_states(step: &Step, members: &[String]) -> Vec<(String, LandingState)> {
    let to = |m: &String| match step {
        Step::Conflict { member } if member == m => LandingState::Conflicted,
        _ => LandingState::Ready,
    };
    match step {
        Step::Conflict { .. } | Step::Drop(_) => {
            members.iter().map(|m| (m.clone(), to(m))).collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        building_s: 1200,
        gating_s: 1800,
    };

    fn built() -> BuildFact {
        BuildFact::Built {
            candidate_ref: "refs/horch/bundle/1".into(),
            candidate_tip: "c1".into(),
        }
    }

    #[test]
    fn each_state_has_1_word() {
        for s in BundleState::ALL {
            assert_eq!(BundleState::parse(s.as_str()), Some(s));
        }
        assert_eq!(BundleState::parse("open"), None);
        let open: Vec<_> = BundleState::ALL
            .into_iter()
            .filter(|s| s.is_open())
            .collect();
        assert_eq!(
            open,
            [
                BundleState::Building,
                BundleState::Gating,
                BundleState::Passed
            ]
        );
    }

    #[test]
    fn building_and_gating_have_a_limit() {
        assert_eq!(due_at(BundleState::Building, 10, &LIMITS), Some(1210));
        assert_eq!(due_at(BundleState::Gating, 10, &LIMITS), Some(1810));
        assert_eq!(due_at(BundleState::Dropped, 10, &LIMITS), None);
        assert!(!overdue(BundleState::Building, 1210, 1209, &LIMITS));
        assert!(overdue(BundleState::Building, 1210, 1210, &LIMITS));
        assert!(!overdue(BundleState::Landed, 0, 9999, &LIMITS));
    }

    #[test]
    fn a_moved_tip_drops_before_any_build_fact() {
        for fact in [
            BuildFact::Pending,
            BuildFact::Abandoned,
            built(),
            BuildFact::Failed,
        ] {
            assert_eq!(building(true, fact), Step::Drop(DropCause::TipMoved));
        }
        assert_eq!(gating(true), Step::Drop(DropCause::TipMoved));
        assert_eq!(gating(false), Step::Stay);
    }

    #[test]
    fn each_build_fact_has_its_step() {
        assert_eq!(building(false, BuildFact::Pending), Step::Stay);
        assert_eq!(building(false, BuildFact::Abandoned), Step::Rebuild);
        assert!(matches!(building(false, built()), Step::Gate { .. }));
        assert_eq!(
            building(false, BuildFact::Failed),
            Step::Drop(DropCause::BuildFailed)
        );
        let conflict = BuildFact::Conflict { member: "b".into() };
        assert_eq!(
            building(false, conflict),
            Step::Conflict { member: "b".into() }
        );
    }

    #[test]
    fn a_conflict_sends_its_member_to_conflicted_and_the_others_to_ready() {
        let members = ["a".to_string(), "b".to_string(), "c".to_string()];
        let step = Step::Conflict { member: "b".into() };
        assert_eq!(
            member_states(&step, &members),
            [
                ("a".to_string(), LandingState::Ready),
                ("b".to_string(), LandingState::Conflicted),
                ("c".to_string(), LandingState::Ready),
            ]
        );
        let dropped = member_states(&Step::Drop(DropCause::TipMoved), &members);
        assert!(dropped.iter().all(|(_, s)| *s == LandingState::Ready));
        assert!(member_states(&Step::Stay, &members).is_empty());
        assert!(member_states(&Step::Rebuild, &members).is_empty());
    }
}
