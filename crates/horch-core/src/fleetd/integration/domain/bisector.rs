#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pending,
    Passed,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Wait,
    Pass,
    Split {
        left: Vec<String>,
        right: Vec<String>,
    },
    GateFailed {
        member: String,
    },
}

pub fn split(members: &[String]) -> (Vec<String>, Vec<String>) {
    let (left, right) = members.split_at(members.len() / 2);
    (left.to_vec(), right.to_vec())
}

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
