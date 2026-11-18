//! The branch invariants: orphan branches and the deletion guard (ticket 44).
//!
//! `docs/specs/fleetd/design.md`, "No work is lost" (items 4 and 5), I11 and
//! I12; `preservation-04-behavior.md`, row W6:
//! - I11: each fleet branch in git has a `branches` row. A fleet branch is
//!   a branch below `fleet/`, or a task branch `<run branch>-T<seq>-<name>`
//!   of the branch of a feature (Q4 of Phase 3). One with no row is adopted
//!   as an orphan row and reported. It is never deleted.
//! - I12: a branch is deleted only when it is landed or rescued, and no
//!   `git stash` entry names it. [`may_delete`] is the guard. A task branch
//!   is deleted only in a state with no exit: `landed`, or `abandoned` with
//!   a rescue ref at its tip. A task branch of a merged feature that is in
//!   neither state can hold work that is not landed, so it is kept.

use super::task_branch::LandingState;

/// The folder of the fleet branches.
pub const FLEET_PREFIX: &str = "fleet/";

/// A feature and its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureBranch {
    pub id: String,
    pub branch: String,
}

/// Whether `name` is `<branch>-T<seq>-<task name>` (Q4 of Phase 3).
fn is_task_branch_of(name: &str, branch: &str) -> bool {
    let Some(rest) = name.strip_prefix(branch).and_then(|r| r.strip_prefix("-T")) else {
        return false;
    };
    let Some((seq, task)) = rest.split_once('-') else {
        return false;
    };
    !seq.is_empty() && seq.bytes().all(|b| b.is_ascii_digit()) && !task.is_empty()
}

/// The feature of the branch `name`: the feature whose branch is `name`,
/// or whose branch has the task branch `name`. The longest feature branch
/// that matches wins.
pub fn feature_of<'a>(name: &str, features: &'a [FeatureBranch]) -> Option<&'a FeatureBranch> {
    features
        .iter()
        .filter(|f| f.branch == name || is_task_branch_of(name, &f.branch))
        .max_by_key(|f| f.branch.len())
}

/// Whether `name` is a fleet branch: below [`FLEET_PREFIX`], or a branch
/// of 1 of `features`.
pub fn is_fleet_branch(name: &str, features: &[FeatureBranch]) -> bool {
    name.starts_with(FLEET_PREFIX) || feature_of(name, features).is_some()
}

/// The facts of 1 task branch that the deletion guard reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeletionFacts {
    /// The row was adopted as an orphan (I11).
    pub orphan: bool,
    /// The landing state of the row, `None` for a branch with no landing
    /// state.
    pub landing: Option<LandingState>,
    /// A rescue ref of the row is at the tip of the branch.
    pub rescued: bool,
    /// A `git stash` entry names the branch (W6).
    pub stashed: bool,
    /// A worktree that is not removed holds the branch. `git branch -D`
    /// refuses such a branch too.
    pub in_worktree: bool,
}

/// Why the guard keeps a branch.
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

/// The deletion guard (I11, I12): `Ok` only when the branch may be
/// deleted.
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
