//! The rescue of a worktree: its ref name, its verification and its
//! needs-you item. No I/O.
//!
//! `docs/specs/fleetd/design.md`, "Rescue before any cleanup", RR-9 and
//! RR-16; MAP.md of fleetd-p3, Q5 and Q6.

/// The namespace of each rescue ref.
pub const REF_PREFIX: &str = "refs/horch/rescue/";

/// The `kind` column of the needs-you item of a failed rescue.
pub const NEEDS_YOU_KIND: &str = "rescue_failed";

/// Whether `id` can name a rescue ref, a manifest and an index file: 1 to
/// 128 ASCII letters, digits, `.`, `_` and `-`, with no `..`, not starting
/// with `.` or `-`, and not ending with `.lock`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && !id.starts_with(['.', '-'])
        && !id.contains("..")
        && !id.ends_with(".lock")
}

/// The folder of the rescue refs of the worktree `id`, with its last `/`.
pub fn ref_folder(id: &str) -> String {
    format!("{REF_PREFIX}{id}/")
}

/// The rescue ref `n` of the worktree `id`.
pub fn ref_name(id: &str, n: u64) -> String {
    format!("{}{n}", ref_folder(id))
}

/// The next `n` of the worktree `id`: 1 + the highest `n` of `refs`, the
/// full names of the refs that exist. A name that is not a rescue ref of
/// `id` with a number does not count.
pub fn next_n(id: &str, refs: &[String]) -> u64 {
    let folder = ref_folder(id);
    refs.iter()
        .filter_map(|r| r.strip_prefix(&folder)?.parse::<u64>().ok())
        .max()
        .map_or(1, |n| n.saturating_add(1))
}

/// The message of the rescue commit of the worktree `id`.
pub fn commit_message(id: &str, attempt: Option<&str>) -> String {
    format!("horch rescue {id} attempt {}", attempt.unwrap_or("none"))
}

/// The key of the needs-you item of a failed rescue of the worktree `id`.
pub fn needs_you_key(id: &str) -> String {
    format!("{NEEDS_YOU_KIND}:{id}")
}

/// The facts that verify a rescue (step 4). A fact that git could not give
/// is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The rescue commit.
    pub commit: String,
    /// The tree of the private index of step 1.
    pub saved_tree: String,
    /// The HEAD of the tree at step 1.
    pub head_before: String,
    /// What the rescue ref names when it is read back.
    pub read_back: Option<String>,
    /// The tree of the rescue commit.
    pub commit_tree: Option<String>,
    /// The parent of the rescue commit.
    pub commit_parent: Option<String>,
    /// The tree of a second new private index of the folder.
    pub disk_tree: Option<String>,
    /// The HEAD of the tree at step 4.
    pub head_after: Option<String>,
}

/// `Ok` when `f` shows that the rescue ref holds exactly the folder on its
/// HEAD; else the first fact that fails.
pub fn verify(f: &Facts) -> Result<(), String> {
    let checks = [
        (
            "the rescue ref reads back the rescue commit",
            &f.read_back,
            &f.commit,
        ),
        (
            "the rescue commit holds the saved tree",
            &f.commit_tree,
            &f.saved_tree,
        ),
        (
            "the parent of the rescue commit is the HEAD",
            &f.commit_parent,
            &f.head_before,
        ),
        (
            "the files on disk are the rescue commit",
            &f.disk_tree,
            &f.saved_tree,
        ),
        (
            "the HEAD did not move during the rescue",
            &f.head_after,
            &f.head_before,
        ),
    ];
    for (rule, fact, want) in checks {
        match fact {
            None => return Err(format!("{rule}: git gave no fact")),
            Some(got) if got != want => return Err(format!("{rule}: {got} is not {want}")),
            Some(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            commit: "c1".into(),
            saved_tree: "t1".into(),
            head_before: "h1".into(),
            read_back: Some("c1".into()),
            commit_tree: Some("t1".into()),
            commit_parent: Some("h1".into()),
            disk_tree: Some("t1".into()),
            head_after: Some("h1".into()),
        }
    }

    #[test]
    fn next_n_is_1_plus_the_highest_n_of_the_tree() {
        let refs = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(next_n("wt1", &[]), 1);
        assert_eq!(
            next_n(
                "wt1",
                &refs(&[
                    "refs/horch/rescue/wt1/1",
                    "refs/horch/rescue/wt1/3",
                    "refs/horch/rescue/wt1/x",
                    "refs/horch/rescue/wt10/9",
                    "refs/horch/rescue/wt1/2/a",
                ])
            ),
            4
        );
    }

    #[test]
    fn verify_passes_only_when_each_fact_agrees() {
        assert_eq!(verify(&facts()), Ok(()));
        let cases: [fn(&mut Facts); 10] = [
            |f| f.read_back = Some("c2".into()),
            |f| f.read_back = None,
            |f| f.commit_tree = Some("t2".into()),
            |f| f.commit_tree = None,
            |f| f.commit_parent = Some("h2".into()),
            |f| f.commit_parent = None,
            |f| f.disk_tree = Some("t2".into()),
            |f| f.disk_tree = None,
            |f| f.head_after = Some("h2".into()),
            |f| f.head_after = None,
        ];
        for (n, change) in cases.iter().enumerate() {
            let mut f = facts();
            change(&mut f);
            assert!(verify(&f).is_err(), "case {n}");
        }
    }

    #[test]
    fn valid_id_refuses_each_name_that_leaves_its_folder() {
        for ok in ["wt1", "T-0007-03", "a.b_c", "0f93854d-8e91"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in [
            "", "a/b", "..", "a..b", ".a", "-a", "a.lock", "a b", "a\\b", "é",
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
        assert!(!valid_id(&"a".repeat(129)));
    }

    #[test]
    fn names_of_a_rescue() {
        assert_eq!(ref_name("wt1", 2), "refs/horch/rescue/wt1/2");
        assert_eq!(needs_you_key("wt1"), "rescue_failed:wt1");
        assert_eq!(
            commit_message("wt1", Some("A2")),
            "horch rescue wt1 attempt A2"
        );
        assert_eq!(commit_message("wt1", None), "horch rescue wt1 attempt none");
    }
}
