//! The states of a bundle and its members (ticket 29).
//!
//! `docs/specs/fleetd/design.md`, "The bundle", "The bundle state machine"
//! (`building`, `gating`) and "A moved tip drops a bundle":
//! - [`building`]: a passed build goes to `gating`, a conflict sends its
//!   member to `conflicted` and drops the bundle, another failure or a
//!   moved feature tip drops it, and an abandoned build runs again.
//! - [`gating`]: a moved feature tip drops the bundle. The verdict of the
//!   fast gate is for the tickets of `passed` (31) and `failed` (30).
//! - [`member_states`]: where each member goes when its bundle leaves.
//! - [`limit_s`] and [`due_at`]: the time limit of `building` and `gating`
//!   is the limit of its job plus `landing_job_margin_s`.

use crate::fleetd::integration::domain::task_branch::LandingState;

/// The state of a bundle.
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

    /// The word of the state in `bundles.state`.
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

    /// The state of the word `word`.
    pub fn parse(word: &str) -> Option<BundleState> {
        Self::ALL.into_iter().find(|s| s.as_str() == word)
    }

    /// `building`, `gating` and `passed` are not ended: a task branch is a
    /// member of at most 1 bundle in these states (I12).
    pub fn is_open(self) -> bool {
        matches!(
            self,
            BundleState::Building | BundleState::Gating | BundleState::Passed
        )
    }
}

/// The time limits of the bundle states, in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The limit of the `bundle_build` job plus `landing_job_margin_s`.
    pub building_s: u64,
    /// The limit of the `fast_gate` job plus `landing_job_margin_s`.
    pub gating_s: u64,
}

/// The time limit of a bundle in `state`, or `None` for a state with no
/// limit here.
pub fn limit_s(state: BundleState, limits: &Limits) -> Option<u64> {
    match state {
        BundleState::Building => Some(limits.building_s),
        BundleState::Gating => Some(limits.gating_s),
        _ => None,
    }
}

/// The due time of a bundle that enters `state` at the fleet clock
/// `entered`, or `None` for a state with no limit here.
pub fn due_at(state: BundleState, entered: i64, limits: &Limits) -> Option<i64> {
    let limit = i64::try_from(limit_s(state, limits)?).unwrap_or(i64::MAX);
    Some(entered.saturating_add(limit))
}

/// Whether a bundle in `state`, due at `due_at`, is past its limit at the
/// fleet clock `clock`.
pub fn overdue(state: BundleState, due_at: i64, clock: i64, limits: &Limits) -> bool {
    limit_s(state, limits).is_some() && clock >= due_at
}

/// What the newest run of the `bundle_build` job of a bundle says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildFact {
    /// The run is `queued` or `running`.
    Pending,
    /// The run ended `abandoned`: it has no result.
    Abandoned,
    /// The candidate ref names the candidate tip.
    Built {
        candidate_ref: String,
        candidate_tip: String,
    },
    /// The merge of `member` conflicts.
    Conflict { member: String },
    /// The run failed for another cause.
    Failed,
}

/// Why a bundle is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropCause {
    /// The tip of the feature branch is not the base tip of the bundle.
    TipMoved,
    /// The build failed for a cause that is not a conflict.
    BuildFailed,
}

/// What a pass does with 1 bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// The bundle stays in its state.
    Stay,
    /// Queue a new run of the build job: the last run was abandoned.
    Rebuild,
    /// Record the candidate, move to `gating` and queue the fast gate.
    Gate {
        candidate_ref: String,
        candidate_tip: String,
    },
    /// `member` goes to `conflicted`, the others to `ready`, and the
    /// bundle to `dropped`.
    Conflict { member: String },
    /// The bundle goes to `dropped`, and each member to `ready`.
    Drop(DropCause),
}

/// The step of a `building` bundle. A moved feature tip comes first: a
/// candidate on an old tip is never gated.
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

/// The step of a `gating` bundle: a moved feature tip drops it.
pub fn gating(tip_moved: bool) -> Step {
    if tip_moved {
        Step::Drop(DropCause::TipMoved)
    } else {
        Step::Stay
    }
}

/// Where each member of `members` goes after `step`: an empty list when
/// the members stay `in_bundle`.
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
