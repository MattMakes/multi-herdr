//! The class of a rescue ref (ticket 24). No I/O.
//!
//! `docs/specs/fleetd/design.md`, "No work is lost" (Rescue refs have 2
//! classes, and fleetd owns both), I10, FM-40, RR-16; MAP.md of fleetd-p4,
//! Q16 and Q17.
//!
//! A rescue ref of a branch is `refs/horch/rescue/branch/<name>/<n>`, with
//! each `/` of the branch name as `-`. A rescue ref of a worktree is
//! `refs/horch/rescue/<worktree id>/<n>` (Phase 3). fleetd decides the class
//! of each rescue ref by content, each time it reads the ref: `superseded`
//! when its changes are in a newer tip of its branch or in a landed branch,
//! else `unique`. Only `superseded` expires.

use super::rescue::{self, REF_PREFIX};

/// The folder of the rescue refs of branches, below [`REF_PREFIX`].
pub const BRANCH_FOLDER: &str = "branch";

/// The `fleet.toml` key of the budget of the `unique` class.
pub const BUDGET_KEY: &str = "rescue_unique_budget_mb";

/// The key and the kind of the needs-you item of the budget.
pub const NEEDS_YOU_KEY: &str = "rescue_unique_budget";

/// The feature states after which a `superseded` ref can expire.
pub const ENDED: [&str; 2] = ["merged", "abandoned"];

const MB: u64 = 1024 * 1024;
const DAY_S: i64 = 24 * 60 * 60;

/// The class of a rescue ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// It holds work that is in no landed branch. Only you delete it.
    Unique,
    /// Its work is in a newer tip of its branch or in a landed branch.
    Superseded,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Unique => "unique",
            Class::Superseded => "superseded",
        }
    }
}

/// What a rescue ref is a rescue of.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefOwner {
    /// A branch, by the folder name: the branch name with `-` for `/`.
    Branch(String),
    /// A worktree, by its id.
    Worktree(String),
}

/// A rescue ref, parsed from its full name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub owner: RefOwner,
    pub n: u64,
}

/// The branch of the owner of a rescue ref, and its feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub branch: String,
    pub feature: Option<FeatureRefs>,
}

/// The id and the branches of a feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRefs {
    pub id: String,
    /// The feature branch; `None` when the feature has none.
    pub branch: Option<String>,
    pub primary: String,
}

/// The folder name of the rescue refs of `branch`: each `/` as `-`. `None`
/// when that name cannot name a ref folder (the rule of
/// [`rescue::valid_id`]).
pub fn branch_folder_name(branch: &str) -> Option<String> {
    let name = branch.replace('/', "-");
    rescue::valid_id(&name).then_some(name)
}

/// The id of the rescue refs of `branch` for [`rescue::ref_name`] and
/// [`rescue::next_n`]: `branch/<folder name>`.
pub fn branch_ref_id(branch: &str) -> Option<String> {
    Some(format!("{BRANCH_FOLDER}/{}", branch_folder_name(branch)?))
}

/// The owner and the number of the rescue ref `name`, or `None` when
/// `name` is not a rescue ref with a number.
pub fn parse(name: &str) -> Option<Parsed> {
    let below = name.strip_prefix(REF_PREFIX)?;
    let (folder, n) = below.rsplit_once('/')?;
    let n = n.parse::<u64>().ok()?;
    let owner = match folder.split_once('/') {
        Some((BRANCH_FOLDER, branch)) if rescue::valid_id(branch) => {
            RefOwner::Branch(branch.to_string())
        }
        None if rescue::valid_id(folder) => RefOwner::Worktree(folder.to_string()),
        _ => return None,
    };
    Some(Parsed { owner, n })
}

/// The by-content facts of 1 rescue ref. A fact that git could not give is
/// `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Content {
    /// Its changes are in a newer tip of its branch.
    pub in_newer_tip: Option<bool>,
    /// Its changes are in a landed branch.
    pub in_landed: Option<bool>,
}

/// The class of a ref with the facts `c`. A fact that is not known never
/// makes a ref `superseded`.
pub fn classify(c: &Content) -> Class {
    if c.in_newer_tip == Some(true) || c.in_landed == Some(true) {
        Class::Superseded
    } else {
        Class::Unique
    }
}

/// Whether a ref of `class` expires at `now_s`: it is `superseded`, its
/// feature ended at `ended_s`, and `retention_days` have passed since then.
/// Times are Unix seconds. A ref with no ended feature never expires.
pub fn expired(class: Class, ended_s: Option<i64>, now_s: i64, retention_days: u64) -> bool {
    let keep_s = i64::try_from(retention_days)
        .ok()
        .and_then(|d| d.checked_mul(DAY_S));
    match (ended_s, keep_s) {
        (Some(ended_s), Some(keep_s)) => {
            class == Class::Superseded && now_s.saturating_sub(ended_s) >= keep_s
        }
        _ => false,
    }
}

/// Whether `unique_bytes` is over the budget of `budget_mb` megabytes.
pub fn over_budget(unique_bytes: u64, budget_mb: u64) -> bool {
    unique_bytes > budget_mb.saturating_mul(MB)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_folder_name_has_dashes_for_slashes() {
        assert_eq!(
            branch_folder_name("fleet/r1/T-1-x").as_deref(),
            Some("fleet-r1-T-1-x")
        );
        assert_eq!(
            branch_ref_id("fleet/r1").as_deref(),
            Some("branch/fleet-r1")
        );
        assert_eq!(
            rescue::ref_name(&branch_ref_id("fleet/r1").unwrap(), 2),
            "refs/horch/rescue/branch/fleet-r1/2"
        );
        for bad in ["", "a b", "a..b", "-a", "a.lock", "é"] {
            assert_eq!(branch_folder_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn parse_tells_branch_refs_from_worktree_refs() {
        assert_eq!(
            parse("refs/horch/rescue/branch/fleet-r1/3"),
            Some(Parsed {
                owner: RefOwner::Branch("fleet-r1".into()),
                n: 3
            })
        );
        assert_eq!(
            parse("refs/horch/rescue/T-0007-03/1"),
            Some(Parsed {
                owner: RefOwner::Worktree("T-0007-03".into()),
                n: 1
            })
        );
        for bad in [
            "refs/horch/rescue/wt1/x",
            "refs/horch/rescue/wt1",
            "refs/horch/rescue/a/b/1",
            "refs/horch/rescue/branch/../1",
            "refs/heads/wt1/1",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_a_known_fact_makes_a_ref_superseded() {
        let c = |a, b| Content {
            in_newer_tip: a,
            in_landed: b,
        };
        assert_eq!(classify(&c(Some(true), None)), Class::Superseded);
        assert_eq!(classify(&c(None, Some(true))), Class::Superseded);
        assert_eq!(classify(&c(Some(false), Some(false))), Class::Unique);
        assert_eq!(classify(&c(None, None)), Class::Unique);
        assert_eq!(classify(&c(Some(false), None)), Class::Unique);
    }

    #[test]
    fn only_a_superseded_ref_of_an_ended_feature_expires_after_its_days() {
        let day = DAY_S;
        assert!(!expired(Class::Superseded, Some(0), 30 * day - 1, 30));
        assert!(expired(Class::Superseded, Some(0), 30 * day, 30));
        assert!(!expired(Class::Unique, Some(0), 3650 * day, 30));
        assert!(!expired(Class::Superseded, None, 3650 * day, 30));
        assert!(!expired(Class::Superseded, Some(0), i64::MAX, u64::MAX));
    }

    #[test]
    fn over_budget_is_strictly_more_than_the_megabytes() {
        assert!(!over_budget(MB, 1));
        assert!(over_budget(MB + 1, 1));
        assert!(!over_budget(u64::MAX, u64::MAX));
    }
}
