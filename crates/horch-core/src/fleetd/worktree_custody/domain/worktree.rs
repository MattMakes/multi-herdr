//! The decisions of the reconciler and invariant I6. No I/O.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" ("The reconciler compares three views" and its 4 rules), "Ledger"
//! (rule 3, I6), RR-10, FM-25, FM-26, FM-27; MAP.md of fleetd-p3, Q4, Q8,
//! Q9, Q10, Q11, Q15 and Q23. Each function is 1 rule: it takes the values
//! that the application read and gives what to write.

use std::collections::BTreeSet;

use super::removal::QUARANTINE_PREFIX;

/// What the folder scan of part 1 saw at the path of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folder {
    Present,
    Missing,
    /// The scan did not reach the row in this pass, or could not read the
    /// path. The folder is not known to be missing.
    Unknown,
}

/// A tree that a view shows: a folder under `<project>/.worktrees/` of a
/// run, or an entry of `git worktree list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The repository of the view.
    pub repo: String,
    pub path: String,
    /// The short name of its branch, when git gives one.
    pub branch: Option<String>,
    /// The main checkout of the repository: never a tree of custody.
    pub main: bool,
    /// An entry that git can prune: its folder is gone, so nothing is on
    /// disk to adopt.
    pub gone: bool,
}

/// The branch of 1 run of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunBranch {
    pub repo: String,
    pub run_id: String,
    pub branch: String,
}

/// A task of a run, by its number: `T<seq>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRef {
    pub run_id: String,
    pub seq: i64,
}

/// A tree with no row, to adopt as `orphaned`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adopt {
    pub repo: String,
    pub path: String,
    /// The short name of its branch; empty when git gives none.
    pub branch: String,
    /// The task that its branch names, if any (Q15).
    pub task: Option<TaskRef>,
}

/// The trees of `seen` that no row owns, each once, in the order seen
/// (FM-26, RR-10). `known` holds the path of each row. The main checkout,
/// a prunable entry and a quarantine folder of a removal are not trees to
/// adopt.
pub fn adoptions(known: &BTreeSet<&str>, seen: &[Seen], runs: &[RunBranch]) -> Vec<Adopt> {
    let mut taken: BTreeSet<&str> = known.clone();
    let mut out = Vec::new();
    for s in seen {
        if s.main || s.gone || quarantine(&s.path) || !taken.insert(s.path.as_str()) {
            continue;
        }
        let branch = s.branch.clone().unwrap_or_default();
        let task = task_of_branch(&s.repo, &branch, runs);
        out.push(Adopt {
            repo: s.repo.clone(),
            path: s.path.clone(),
            branch,
            task,
        });
    }
    out
}

/// Whether the last part of `path` names a quarantine folder of a removal.
fn quarantine(path: &str) -> bool {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .is_some_and(|name| name.starts_with(QUARANTINE_PREFIX))
}

/// The task that `branch` of the repository `repo` names:
/// `<run branch>-T<seq>-<name>` of a run of `repo` (Q4). The longest run
/// branch that matches wins; of 2 runs with the same branch, the first in
/// `runs` wins.
pub fn task_of_branch(repo: &str, branch: &str, runs: &[RunBranch]) -> Option<TaskRef> {
    runs.iter()
        .filter(|r| r.repo == repo && !r.branch.is_empty())
        .filter_map(|r| {
            let rest = branch.strip_prefix(r.branch.as_str())?.strip_prefix("-T")?;
            let (seq, name) = rest.split_once('-')?;
            if name.is_empty() || !seq.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some((r, seq.parse::<i64>().ok()?))
        })
        .min_by_key(|(r, _)| std::cmp::Reverse(r.branch.len()))
        .map(|(r, seq)| TaskRef {
            run_id: r.run_id.clone(),
            seq,
        })
}

/// The owner attempt of an `active` row, as its ledger row says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// `launching` or `live`.
    Holds,
    Ended,
    /// No attempt row has the id.
    Missing,
}

/// The facts of the owner of an `active` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    /// `None` when the row names no owner attempt.
    pub attempt: Option<Attempt>,
    /// The worker of the row is live (Q23).
    pub worker_live: bool,
    /// The task is parked by `done --unlanded`.
    pub task_parked: bool,
    pub task_cancelled: bool,
}

/// Where an `active` row goes when its owner is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerGone {
    /// `created`, with no owner: the next attempt of the task takes it.
    Created,
    /// `orphaned`, for this cause (FM-25).
    Orphaned(&'static str),
}

/// The cause of a tree whose task is cancelled.
pub const CANCELLED: &str = "its task is cancelled";
/// The cause of a tree whose owner attempt ended.
pub const OWNER_ENDED: &str = "its owner attempt ended";
/// The cause of a tree with no owner attempt and no live worker.
pub const NO_LIVE_OWNER: &str = "it has no owner attempt and its worker is not live";

/// What happens to an `active` row with the owner `o`; `None` while the
/// owner holds it. A task parked by `done --unlanded` keeps its tree as
/// `created`. Each other end orphans the tree: the outcome of the attempt
/// is not read, and `orphaned` is the safe state, as a rescue comes first
/// (Q10; the choice of the orchestrator for ticket 09).
pub fn owner_gone(o: &Owner) -> Option<OwnerGone> {
    if o.task_cancelled {
        return Some(OwnerGone::Orphaned(CANCELLED));
    }
    let cause = match o.attempt {
        Some(Attempt::Holds) => return None,
        Some(Attempt::Ended | Attempt::Missing) => OWNER_ENDED,
        None if o.worker_live => return None,
        None => NO_LIVE_OWNER,
    };
    Some(if o.task_parked {
        OwnerGone::Created
    } else {
        OwnerGone::Orphaned(cause)
    })
}

/// Whether a row in `state` has lost its folder: the scan saw it missing,
/// `git worktree list` does not show it, and it is not set aside by a
/// removal (FM-27). A `reserved` row has no folder yet, and a `removed`
/// row has none any more.
pub fn vanished(state: &str, folder: Folder, in_git: bool, set_aside: bool) -> bool {
    !matches!(state, "reserved" | "removed") && folder == Folder::Missing && !in_git && !set_aside
}

/// Why the door move `orphaned` to `removed` of a row with no folder is
/// refused, or `None`. Only the operator decision ARCHIVE or DELETE of
/// triage makes this move; it is the answer to the FM-27 escalation. The
/// folder must be missing, `git worktree list` must not name its path, and
/// the archive or trash ref, read back, must hold the tip of its branch
/// (`holds_tip`; `None` for a row with no branch).
pub fn settle_refusal(
    folder: Folder,
    in_git: bool,
    holds_tip: Option<bool>,
) -> Option<&'static str> {
    if folder != Folder::Missing {
        return Some("its folder is not known to be gone");
    }
    if in_git {
        return Some("git worktree list still names its path; run git worktree prune");
    }
    if holds_tip == Some(false) {
        return Some("the archive or trash ref does not hold the tip of its branch");
    }
    None
}

/// The `kind` and the event cause of a vanished folder.
pub const VANISHED_KIND: &str = "worktree_vanished";

/// The key of the needs-you item of the vanished folder of the row `id`.
pub fn vanished_key(id: &str) -> String {
    format!("{VANISHED_KIND}:{id}")
}

/// A job of a tree whose ended run the reconciler observes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeJob {
    Add,
    Rescue,
    Remove,
}

/// The job that the reconciler queues for a row in `state`: a rescue of an
/// `orphaned` row, a removal of a `landed` row (FM-25, FM-27, Q8). A
/// verified rescue moves an orphan to `rescued`, so an `orphaned` row has no
/// verified rescue of its state now, even with the ref of an earlier
/// rescue of a reused tree. A `rescued` row is removed only for space.
pub fn to_queue(state: &str) -> Option<TreeJob> {
    match state {
        "orphaned" => Some(TreeJob::Rescue),
        "landed" => Some(TreeJob::Remove),
        _ => None,
    }
}

/// The job whose ended run a row in `state` waits for: the `worktree_add`
/// run of a `reserved` row, the rescue of an `orphaned` or `landing` row,
/// and the removal of a `landed` or `rescued` row.
pub fn awaited(state: &str) -> Option<TreeJob> {
    match state {
        "reserved" => Some(TreeJob::Add),
        "orphaned" | "landing" => Some(TreeJob::Rescue),
        "landed" | "rescued" => Some(TreeJob::Remove),
        _ => None,
    }
}

/// A breach of I6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Breach {
    /// `git worktree list` or the work directory shows a tree with no row.
    NoRow { repo: String, path: String },
    /// A row that is not `removed` has no folder.
    NoFolder { id: String },
    /// A `reserved` row has no `worktree_add` run: the row and its run are
    /// written in 1 transaction, so this cannot happen.
    ReservedWithNoJob { id: String },
    /// A `removed` row had neither landed work nor a rescue ref.
    RemovedUnsaved { id: String },
}

/// Whether a `removed` row with today's `status` and `rescue_ref` was
/// removed with its work saved: on a rescue ref, landed, or on an archive
/// or trash ref of triage.
pub fn removal_saved(status: &str, rescue_ref: Option<&str>) -> bool {
    rescue_ref.is_some_and(|r| !r.is_empty())
        || matches!(status, "landed" | "cleaned" | "archived" | "trashed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(path: &str, branch: Option<&str>) -> Seen {
        Seen {
            repo: "/r".into(),
            path: path.into(),
            branch: branch.map(str::to_string),
            main: false,
            gone: false,
        }
    }

    fn run(id: &str, branch: &str) -> RunBranch {
        RunBranch {
            repo: "/r".into(),
            run_id: id.into(),
            branch: branch.into(),
        }
    }

    #[test]
    fn adoptions_skip_known_main_gone_quarantine_and_twice_seen_trees() {
        let known: BTreeSet<&str> = ["/r/.worktrees/a"].into();
        let mut main = seen("/r", Some("main"));
        main.main = true;
        let mut gone = seen("/r/.worktrees/g", None);
        gone.gone = true;
        let views = [
            main,
            seen("/r/.worktrees/a", None),
            gone,
            seen("/r/.worktrees/.horch-removing-w1", None),
            seen("/r/.worktrees/b", Some("x")),
            seen("/r/.worktrees/b", None),
        ];
        let adopt = adoptions(&known, &views, &[]);
        assert_eq!(
            adopt,
            vec![Adopt {
                repo: "/r".into(),
                path: "/r/.worktrees/b".into(),
                branch: "x".into(),
                task: None,
            }]
        );
    }

    #[test]
    fn task_of_branch_reads_the_run_branch_the_number_and_a_name() {
        let runs = [run("r1", "fleet/a"), run("r2", "fleet/a-T9-x")];
        let task = |b: &str| task_of_branch("/r", b, &runs);
        assert_eq!(
            task("fleet/a-T12-feat"),
            Some(TaskRef {
                run_id: "r1".into(),
                seq: 12
            })
        );
        // The longest run branch wins.
        assert_eq!(
            task("fleet/a-T9-x-T3-y"),
            Some(TaskRef {
                run_id: "r2".into(),
                seq: 3
            })
        );
        assert_eq!(task("fleet/a-T12"), None);
        assert_eq!(task("fleet/a-T-feat"), None);
        assert_eq!(task("fleet/a-Tx2-feat"), None);
        assert_eq!(task("fleet/b-T1-feat"), None);
        assert_eq!(task_of_branch("/other", "fleet/a-T1-x", &runs), None);
    }

    /// `runs::of_project` gives the newest run first. 2 runs with the same
    /// run branch give the task of the newest (review F2).
    #[test]
    fn task_of_branch_takes_the_first_run_of_a_shared_run_branch() {
        let runs = [run("new", "fleet/a"), run("old", "fleet/a")];
        assert_eq!(
            task_of_branch("/r", "fleet/a-T3-x", &runs),
            Some(TaskRef {
                run_id: "new".into(),
                seq: 3
            })
        );
    }

    fn owner(attempt: Option<Attempt>, worker_live: bool) -> Owner {
        Owner {
            attempt,
            worker_live,
            task_parked: false,
            task_cancelled: false,
        }
    }

    #[test]
    fn owner_gone_orphans_each_end_but_a_parked_task() {
        let gone = |o: Owner| owner_gone(&o);
        assert_eq!(gone(owner(Some(Attempt::Holds), false)), None);
        assert_eq!(gone(owner(None, true)), None);
        assert_eq!(
            gone(owner(Some(Attempt::Ended), true)),
            Some(OwnerGone::Orphaned(OWNER_ENDED))
        );
        assert_eq!(
            gone(owner(Some(Attempt::Missing), true)),
            Some(OwnerGone::Orphaned(OWNER_ENDED))
        );
        assert_eq!(
            gone(owner(None, false)),
            Some(OwnerGone::Orphaned(NO_LIVE_OWNER))
        );
        let parked = Owner {
            task_parked: true,
            ..owner(Some(Attempt::Ended), false)
        };
        assert_eq!(gone(parked), Some(OwnerGone::Created));
        let cancelled = Owner {
            task_cancelled: true,
            ..owner(Some(Attempt::Holds), true)
        };
        assert_eq!(gone(cancelled), Some(OwnerGone::Orphaned(CANCELLED)));
    }

    #[test]
    fn settle_refuses_a_folder_that_exists_or_is_unknown() {
        assert!(settle_refusal(Folder::Present, false, Some(true)).is_some());
        assert!(settle_refusal(Folder::Unknown, false, Some(true)).is_some());
    }

    #[test]
    fn settle_refuses_a_path_that_git_still_lists() {
        assert!(settle_refusal(Folder::Missing, true, Some(true)).is_some());
    }

    #[test]
    fn settle_refuses_a_ref_that_does_not_hold_the_tip() {
        assert!(settle_refusal(Folder::Missing, false, Some(false)).is_some());
    }

    #[test]
    fn settle_permits_a_gone_folder_whose_work_a_ref_holds_or_that_has_no_branch() {
        assert_eq!(settle_refusal(Folder::Missing, false, Some(true)), None);
        assert_eq!(settle_refusal(Folder::Missing, false, None), None);
    }

    #[test]
    fn vanished_needs_a_missing_folder_that_git_and_a_removal_do_not_explain() {
        assert!(vanished("active", Folder::Missing, false, false));
        assert!(!vanished("active", Folder::Unknown, false, false));
        assert!(!vanished("active", Folder::Present, false, false));
        assert!(!vanished("active", Folder::Missing, true, false));
        assert!(!vanished("rescued", Folder::Missing, false, true));
        assert!(!vanished("reserved", Folder::Missing, false, false));
        assert!(!vanished("removed", Folder::Missing, false, false));
    }

    #[test]
    fn to_queue_a_rescue_of_an_orphan_and_a_removal_of_a_landed_tree() {
        assert_eq!(to_queue("orphaned"), Some(TreeJob::Rescue));
        assert_eq!(to_queue("landed"), Some(TreeJob::Remove));
        for state in [
            "reserved", "created", "active", "landing", "rescued", "removed",
        ] {
            assert_eq!(to_queue(state), None, "{state}");
        }
    }

    #[test]
    fn removal_is_saved_by_a_rescue_ref_or_landed_or_triage_work() {
        assert!(removal_saved("open", Some("refs/horch/rescue/w/1")));
        for status in ["landed", "cleaned", "archived", "trashed"] {
            assert!(removal_saved(status, None), "{status}");
        }
        assert!(!removal_saved("open", None));
        assert!(!removal_saved("unlanded", Some("")));
    }
}
