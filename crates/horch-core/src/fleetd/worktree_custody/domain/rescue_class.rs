use super::rescue::{self, REF_PREFIX};

pub const BRANCH_FOLDER: &str = "branch";

pub const BUDGET_KEY: &str = "rescue_unique_budget_mb";

pub const NEEDS_YOU_KEY: &str = "rescue_unique_budget";

pub const ENDED: [&str; 2] = ["merged", "abandoned"];

const MB: u64 = 1024 * 1024;
const DAY_S: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Unique,
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefOwner {
    Branch(String),
    Worktree(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub owner: RefOwner,
    pub n: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub branch: String,
    pub feature: Option<FeatureRefs>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRefs {
    pub id: String,
    pub branch: Option<String>,
    pub primary: String,
}

pub fn branch_folder_name(branch: &str) -> Option<String> {
    let name = branch.replace('/', "-");
    rescue::valid_id(&name).then_some(name)
}

pub fn branch_ref_id(branch: &str) -> Option<String> {
    Some(format!("{BRANCH_FOLDER}/{}", branch_folder_name(branch)?))
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Content {
    pub in_newer_tip: Option<bool>,
    pub in_landed: Option<bool>,
}

pub fn classify(c: &Content) -> Class {
    if c.in_newer_tip == Some(true) || c.in_landed == Some(true) {
        Class::Superseded
    } else {
        Class::Unique
    }
}

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
