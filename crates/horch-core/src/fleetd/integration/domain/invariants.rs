use super::task_branch::LandingState;

pub const FLEET_PREFIX: &str = "fleet/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureBranch {
    pub id: String,
    pub branch: String,
}

fn is_task_branch_of(name: &str, branch: &str) -> bool {
    let Some(rest) = name.strip_prefix(branch).and_then(|r| r.strip_prefix("-T")) else {
        return false;
    };
    let Some((seq, task)) = rest.split_once('-') else {
        return false;
    };
    !seq.is_empty() && seq.bytes().all(|b| b.is_ascii_digit()) && !task.is_empty()
}

pub fn feature_of<'a>(name: &str, features: &'a [FeatureBranch]) -> Option<&'a FeatureBranch> {
    features
        .iter()
        .filter(|f| f.branch == name || is_task_branch_of(name, &f.branch))
        .max_by_key(|f| f.branch.len())
}

pub fn is_fleet_branch(name: &str, features: &[FeatureBranch]) -> bool {
    name.starts_with(FLEET_PREFIX) || feature_of(name, features).is_some()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeletionFacts {
    pub orphan: bool,
    pub landing: Option<LandingState>,
    pub rescued: bool,
    pub stashed: bool,
    pub in_worktree: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Orphan,
    Stashed,
    InWorktree,
    NotSaved,
}

impl Refusal {
    pub fn text(self) -> &'static str {
        match self {
            Refusal::Orphan => "an orphan branch is never deleted",
            Refusal::Stashed => "a git stash entry names the branch",
            Refusal::InWorktree => "a worktree holds the branch",
            Refusal::NotSaved => {
                "the branch is not landed, and not abandoned with a rescue ref at its tip"
            }
        }
    }
}

pub fn may_delete(f: &DeletionFacts) -> Result<(), Refusal> {
    if f.orphan {
        return Err(Refusal::Orphan);
    }
    if f.stashed {
        return Err(Refusal::Stashed);
    }
    if f.in_worktree {
        return Err(Refusal::InWorktree);
    }
    match f.landing {
        Some(LandingState::Landed) => Ok(()),
        Some(LandingState::Abandoned) if f.rescued => Ok(()),
        _ => Err(Refusal::NotSaved),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn features() -> Vec<FeatureBranch> {
        vec![
            FeatureBranch {
                id: "F-1".into(),
                branch: "fleet/r1".into(),
            },
            FeatureBranch {
                id: "F-2".into(),
                branch: "fleet/r1-T9-x".into(),
            },
            FeatureBranch {
                id: "F-3".into(),
                branch: "work".into(),
            },
        ]
    }

    fn id_of(name: &str) -> Option<String> {
        feature_of(name, &features()).map(|f| f.id.clone())
    }

    #[test]
    fn a_task_branch_names_its_feature_and_the_longest_branch_wins() {
        assert_eq!(id_of("fleet/r1").as_deref(), Some("F-1"));
        assert_eq!(id_of("fleet/r1-T12-fix").as_deref(), Some("F-1"));
        assert_eq!(id_of("fleet/r1-T9-x-T1-y").as_deref(), Some("F-2"));
        assert_eq!(id_of("work-T3-a/b").as_deref(), Some("F-3"));
        assert_eq!(id_of("fleet/r1-Tx-fix"), None);
        assert_eq!(id_of("fleet/r1-T12-"), None);
        assert_eq!(id_of("fleet/r1x"), None);
        assert_eq!(id_of("work-T-a"), None);
    }

    #[test]
    fn a_fleet_branch_is_below_fleet_or_a_branch_of_a_feature() {
        let f = features();
        assert!(is_fleet_branch("fleet/anything", &f));
        assert!(is_fleet_branch("work-T1-a", &f));
        assert!(is_fleet_branch("work", &f));
        assert!(!is_fleet_branch("main", &f));
        assert!(!is_fleet_branch("workshop", &f));
    }

    #[test]
    fn i11_an_orphan_is_never_deleted() {
        let f = DeletionFacts {
            orphan: true,
            landing: Some(LandingState::Landed),
            rescued: true,
            ..DeletionFacts::default()
        };
        assert_eq!(may_delete(&f), Err(Refusal::Orphan));
    }

    #[test]
    fn i12_a_stash_entry_blocks_the_deletion() {
        let f = DeletionFacts {
            landing: Some(LandingState::Landed),
            stashed: true,
            ..DeletionFacts::default()
        };
        assert_eq!(may_delete(&f), Err(Refusal::Stashed));
    }

    #[test]
    fn i12_only_a_landed_or_a_rescued_abandoned_branch_is_deleted() {
        for state in LandingState::ALL {
            for rescued in [false, true] {
                let f = DeletionFacts {
                    landing: Some(state),
                    rescued,
                    ..DeletionFacts::default()
                };
                let ok =
                    state == LandingState::Landed || (state == LandingState::Abandoned && rescued);
                assert_eq!(may_delete(&f).is_ok(), ok, "{state:?} rescued={rescued}");
            }
        }
        let feature_branch = DeletionFacts {
            rescued: true,
            ..DeletionFacts::default()
        };
        assert_eq!(may_delete(&feature_branch), Err(Refusal::NotSaved));
        let held = DeletionFacts {
            landing: Some(LandingState::Landed),
            in_worktree: true,
            ..DeletionFacts::default()
        };
        assert_eq!(may_delete(&held), Err(Refusal::InWorktree));
    }
}
