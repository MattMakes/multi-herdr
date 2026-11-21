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
        if s.main || s.gone || !unowned(&taken, &s.path) {
            continue;
        }
        taken.insert(s.path.as_str());
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

/// Whether the tree at `path` is one that no row owns: `known` (the path
/// of each row) does not hold it, and it is not a quarantine folder of a
/// removal.
pub fn unowned(known: &BTreeSet<&str>, path: &str) -> bool {
    !known.contains(path) && !quarantine(path)
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

/// The state of the task of an `active` row, as the owner rule reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskNow {
    /// The verifier checks the result of the attempt.
    Verifying,
    /// The verifier verified the task: `done`.
    Done,
    /// The task waits for its next attempt: `ready`, `blocked_on_user` or
    /// `waiting_quota`.
    Retried,
    /// The task is parked by `done --unlanded`.
    Parked,
    Cancelled,
    /// Each other state, and a row with no task.
    Other,
}

/// The facts of the owner of an `active` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    /// `None` when the row names no owner attempt.
    pub attempt: Option<Attempt>,
    /// The worker of the row is live (Q23).
    pub worker_live: bool,
    pub task: TaskNow,
    /// The owner attempt ended with an outcome of a process that did not
    /// die: [`clean_end`].
    pub ended_clean: bool,
}

/// Whether the attempt outcome `outcome` is a clean end: the verifier
/// rejected the result, or the worker asked, hit a usage limit, has an open
/// CRITICAL item or reached its context limit. A crash, a kill, a time
/// limit, a stall and an exit with no result are a dead owner.
pub fn clean_end(outcome: Option<&str>) -> bool {
    matches!(
        outcome,
        Some("rejected" | "asked" | "quota" | "critical_open" | "context_limit")
    )
}

/// Where an `active` row goes when its owner is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerGone {
    /// `created`, with no owner, for this cause: the next attempt of the
    /// task takes it.
    Created(&'static str),
    /// `landing`: the attempt ended verified.
    Landing,
    /// `orphaned`, for this cause (FM-25).
    Orphaned(&'static str),
}

/// The cause of a tree whose task is cancelled.
pub const CANCELLED: &str = "its task is cancelled";
/// The cause of a tree whose owner attempt ended.
pub const OWNER_ENDED: &str = "its owner attempt ended";
/// The cause of a tree with no owner attempt and no live worker.
pub const NO_LIVE_OWNER: &str = "it has no owner attempt and its worker is not live";
/// The cause of a tree of a task parked by `done --unlanded`.
pub const PARKED: &str = "its task is parked by done --unlanded";
/// The cause of a tree of a task that waits for its next attempt.
pub const RETRIED: &str = "its task waits for its next attempt";
/// The cause of a tree whose attempt ended verified.
pub const VERIFIED: &str = "its attempt ended verified";

/// What happens to an `active` row with the owner `o`; `None` while the
/// owner holds it, or while the verifier checks the task. The design row
/// `active`: "The attempt ends: verified → `landing`; retried → back to
/// `created`, so the next attempt continues in the same tree; owner dead →
/// `orphaned`". A retry is a task that waits for its next attempt after a
/// clean end of the attempt; after a dead owner the tree is orphaned. A
/// task parked by `done --unlanded` keeps its tree as `created` (Q10). A
/// cancelled task, and each other end, orphans the tree: a rescue comes
/// first.
pub fn owner_gone(o: &Owner) -> Option<OwnerGone> {
    if o.task == TaskNow::Cancelled {
        return Some(OwnerGone::Orphaned(CANCELLED));
    }
    let cause = match o.attempt {
        Some(Attempt::Holds) => return None,
        Some(Attempt::Ended | Attempt::Missing) => OWNER_ENDED,
        None if o.worker_live => return None,
        None => NO_LIVE_OWNER,
    };
    match o.task {
        TaskNow::Verifying => None,
        TaskNow::Done => Some(OwnerGone::Landing),
        TaskNow::Retried if o.ended_clean => Some(OwnerGone::Created(RETRIED)),
        TaskNow::Parked => Some(OwnerGone::Created(PARKED)),
        TaskNow::Retried | TaskNow::Cancelled | TaskNow::Other => Some(OwnerGone::Orphaned(cause)),
    }
}

/// What a `landing` row does when its task branch is in the landing state
/// `branch` (`None`: no row of the branch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingTree {
    /// The branch is `landed`: the tree moves to `landed`.
    Landed,
    /// The branch is `abandoned`: fleetd queues a rescue job, and the
    /// verified rescue moves the tree to `rescued`.
    Rescue,
}

/// The design row `landing`: "Its branch is `landed` on the feature branch
/// → `landed`. Its branch is `abandoned` → fleetd queues a rescue job".
/// Each other landing state keeps the tree in `landing`, where a resolver
/// or fix attempt of its branch runs.
pub fn landing_next(branch: Option<&str>) -> Option<LandingTree> {
    match branch {
        Some("landed") => Some(LandingTree::Landed),
        Some("abandoned") => Some(LandingTree::Rescue),
        _ => None,
    }
}

/// Whether the `worktree_list` read of a `landing` row at `seen_at` came
/// after its branch landed at `landed_at`: only then is its HEAD the landed
/// commit. No writer moves the tree between the landing and that read. A
/// branch with no event of its landing (a row that a migration wrote
/// `landed`) landed before each read.
pub fn read_after_landing(seen_at: Option<&str>, landed_at: Option<&str>) -> bool {
    match (seen_at, landed_at) {
        (Some(seen), Some(landed)) => seen >= landed,
        (Some(_), None) => true,
        (None, _) => false,
    }
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
/// rescue of a reused tree. A `rescued` row is removed for space, or when
/// its idea is `merged` (the reconciler reads the idea).
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
            task: TaskNow::Other,
            ended_clean: false,
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
            task: TaskNow::Parked,
            ..owner(Some(Attempt::Ended), false)
        };
        assert_eq!(gone(parked), Some(OwnerGone::Created(PARKED)));
        let cancelled = Owner {
            task: TaskNow::Cancelled,
            ..owner(Some(Attempt::Holds), true)
        };
        assert_eq!(gone(cancelled), Some(OwnerGone::Orphaned(CANCELLED)));
    }

    #[test]
    fn owner_gone_lands_a_verified_tree_and_keeps_a_retried_one() {
        let ended = |task| Owner {
            task,
            ..owner(Some(Attempt::Ended), false)
        };
        assert_eq!(owner_gone(&ended(TaskNow::Done)), Some(OwnerGone::Landing));
        assert_eq!(owner_gone(&ended(TaskNow::Verifying)), None);
        assert_eq!(
            owner_gone(&ended(TaskNow::Retried)),
            Some(OwnerGone::Orphaned(OWNER_ENDED)),
            "a dead owner"
        );
        let clean = Owner {
            ended_clean: true,
            ..ended(TaskNow::Retried)
        };
        assert_eq!(owner_gone(&clean), Some(OwnerGone::Created(RETRIED)));
        let held = Owner {
            task: TaskNow::Done,
            ..owner(Some(Attempt::Holds), true)
        };
        assert_eq!(owner_gone(&held), None);
    }

    #[test]
    fn a_clean_end_is_an_outcome_of_a_process_that_did_not_die() {
        for o in [
            "rejected",
            "asked",
            "quota",
            "critical_open",
            "context_limit",
        ] {
            assert!(clean_end(Some(o)), "{o}");
        }
        for o in [
            "crashed",
            "killed",
            "timed_out",
            "stalled",
            "no_result",
            "done",
        ] {
            assert!(!clean_end(Some(o)), "{o}");
        }
        assert!(!clean_end(None));
    }

    #[test]
    fn landing_next_follows_a_landed_or_abandoned_branch_only() {
        assert_eq!(landing_next(Some("landed")), Some(LandingTree::Landed));
        assert_eq!(landing_next(Some("abandoned")), Some(LandingTree::Rescue));
        for state in [
            "building",
            "verifying",
            "ready",
            "in_bundle",
            "conflicted",
            "gate_failed",
        ] {
            assert_eq!(landing_next(Some(state)), None, "{state}");
        }
        assert_eq!(landing_next(None), None);
    }

    #[test]
    fn a_read_counts_only_at_or_after_the_landing() {
        let at = Some("2026-11-20T10:00:00Z");
        assert!(read_after_landing(Some("2026-11-20T10:00:00Z"), at));
        assert!(read_after_landing(Some("2026-11-20T10:00:01Z"), at));
        assert!(!read_after_landing(Some("2026-11-20T09:59:59Z"), at));
        assert!(!read_after_landing(None, at));
        assert!(read_after_landing(Some("2026-11-20T10:00:00Z"), None));
        assert!(!read_after_landing(None, None));
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
