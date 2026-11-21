use std::collections::BTreeSet;

use super::removal::QUARANTINE_PREFIX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folder {
    Present,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub repo: String,
    pub path: String,
    pub branch: Option<String>,
    pub main: bool,
    pub gone: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunBranch {
    pub repo: String,
    pub run_id: String,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRef {
    pub run_id: String,
    pub seq: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adopt {
    pub repo: String,
    pub path: String,
    pub branch: String,
    pub task: Option<TaskRef>,
}

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

pub fn unowned(known: &BTreeSet<&str>, path: &str) -> bool {
    !known.contains(path) && !quarantine(path)
}

fn quarantine(path: &str) -> bool {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .is_some_and(|name| name.starts_with(QUARANTINE_PREFIX))
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    Holds,
    Ended,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskNow {
    Verifying,
    Done,
    Retried,
    Parked,
    Cancelled,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    pub attempt: Option<Attempt>,
    pub worker_live: bool,
    pub task: TaskNow,
    pub ended_clean: bool,
}

pub fn clean_end(outcome: Option<&str>) -> bool {
    matches!(
        outcome,
        Some("rejected" | "asked" | "quota" | "critical_open" | "context_limit")
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerGone {
    Created(&'static str),
    Landing,
    Orphaned(&'static str),
}

pub const CANCELLED: &str = "its task is cancelled";
pub const OWNER_ENDED: &str = "its owner attempt ended";
pub const NO_LIVE_OWNER: &str = "it has no owner attempt and its worker is not live";
pub const PARKED: &str = "its task is parked by done --unlanded";
pub const RETRIED: &str = "its task waits for its next attempt";
pub const VERIFIED: &str = "its attempt ended verified";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingTree {
    Landed,
    Rescue,
}

pub fn landing_next(branch: Option<&str>) -> Option<LandingTree> {
    match branch {
        Some("landed") => Some(LandingTree::Landed),
        Some("abandoned") => Some(LandingTree::Rescue),
        _ => None,
    }
}

pub fn read_after_landing(seen_at: Option<&str>, landed_at: Option<&str>) -> bool {
    match (seen_at, landed_at) {
        (Some(seen), Some(landed)) => seen >= landed,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

pub fn vanished(state: &str, folder: Folder, in_git: bool, set_aside: bool) -> bool {
    !matches!(state, "reserved" | "removed") && folder == Folder::Missing && !in_git && !set_aside
}

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

pub const VANISHED_KIND: &str = "worktree_vanished";

pub fn vanished_key(id: &str) -> String {
    format!("{VANISHED_KIND}:{id}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeJob {
    Add,
    Rescue,
    Remove,
}

pub fn to_queue(state: &str) -> Option<TreeJob> {
    match state {
        "orphaned" => Some(TreeJob::Rescue),
        "landed" => Some(TreeJob::Remove),
        _ => None,
    }
}

pub fn awaited(state: &str) -> Option<TreeJob> {
    match state {
        "reserved" => Some(TreeJob::Add),
        "orphaned" | "landing" => Some(TreeJob::Rescue),
        "landed" | "rescued" => Some(TreeJob::Remove),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Breach {
    NoRow { repo: String, path: String },
    NoFolder { id: String },
    ReservedWithNoJob { id: String },
    RemovedUnsaved { id: String },
}

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
