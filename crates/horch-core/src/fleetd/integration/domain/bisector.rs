//! The bisector of a failed bundle (ticket 30).
//!
//! `docs/specs/fleetd/design.md`, "The bundle" and "The bundle state
//! machine" (`failed`): on a failure of its fast gate, a bundle moves to
//! `failed`, and fleetd bisects it. Each half is a child bundle, built and
//! gated in the same way. A bundle of 1 member that fails finds its failing
//! branch, and that branch goes to `gate_failed`. A bundle that passes goes
//! to `passed`, and its members land.
//! - [`split`]: the 2 halves of the members, in queue order.
//! - [`decide`]: what a `gating` bundle does with the verdict of its fast
//!   gate.

/// The verdict of the fast gate of a bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The gate has no verdict yet.
    Pending,
    Passed,
    Failed,
}

/// What a `gating` bundle does with the verdict of its fast gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The gate has no verdict yet: the bundle stays `gating`.
    Wait,
    /// The bundle goes to `passed`, and its members land.
    Pass,
    /// The bundle goes to `failed`, and each half is 1 child bundle.
    Split {
        left: Vec<String>,
        right: Vec<String>,
    },
    /// The bundle goes to `failed`, and its 1 member is the failing branch:
    /// it goes to `gate_failed`.
    GateFailed { member: String },
}

/// The 2 halves of `members`, in queue order. The first half has the
/// smaller count when the count is odd.
pub fn split(members: &[String]) -> (Vec<String>, Vec<String>) {
    let (left, right) = members.split_at(members.len() / 2);
    (left.to_vec(), right.to_vec())
}

/// The decision of a `gating` bundle of `members` with the fast gate
/// verdict `verdict`. A failed bundle with no member has no branch to find,
/// so it waits; the time limit of `gating` names it.
pub fn decide(verdict: Verdict, members: &[String]) -> Decision {
    match (verdict, members) {
        (Verdict::Pending, _) | (Verdict::Failed, []) => Decision::Wait,
        (Verdict::Passed, _) => Decision::Pass,
        (Verdict::Failed, [member]) => Decision::GateFailed {
            member: member.clone(),
        },
        (Verdict::Failed, _) => {
            let (left, right) = split(members);
            Decision::Split { left, right }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("b{i}")).collect()
    }

    #[test]
    fn split_of_4_is_2_and_2_in_queue_order() {
        let (left, right) = split(&names(4));
        assert_eq!(left, ["b0", "b1"]);
        assert_eq!(right, ["b2", "b3"]);
    }

    #[test]
    fn split_of_3_is_1_and_2_in_queue_order() {
        let (left, right) = split(&names(3));
        assert_eq!(left, ["b0"]);
        assert_eq!(right, ["b1", "b2"]);
    }

    #[test]
    fn split_keeps_each_member_once_and_both_halves_are_smaller() {
        for n in 2..=9 {
            let members = names(n);
            let (left, right) = split(&members);
            assert!(!left.is_empty() && !right.is_empty(), "n = {n}");
            let joined: Vec<String> = left.into_iter().chain(right).collect();
            assert_eq!(joined, members, "n = {n}");
        }
    }

    #[test]
    fn failed_bundle_of_2_or_more_splits() {
        assert_eq!(
            decide(Verdict::Failed, &names(4)),
            Decision::Split {
                left: names(2),
                right: vec!["b2".into(), "b3".into()],
            }
        );
    }

    #[test]
    fn failed_bundle_of_1_finds_its_failing_branch() {
        assert_eq!(
            decide(Verdict::Failed, &names(1)),
            Decision::GateFailed {
                member: "b0".into()
            }
        );
    }

    #[test]
    fn passed_bundle_passes_and_pending_waits() {
        for n in 0..=3 {
            assert_eq!(decide(Verdict::Passed, &names(n)), Decision::Pass);
            assert_eq!(decide(Verdict::Pending, &names(n)), Decision::Wait);
        }
    }

    #[test]
    fn failed_bundle_with_no_member_waits() {
        assert_eq!(decide(Verdict::Failed, &[]), Decision::Wait);
    }
}
