//! The verdict of the verifier on an attempt (ticket 19).
//!
//! `docs/specs/fleetd/design.md`, "Architecture and roles" (Verifier; Done
//! for each kind of task), "The task state machine" (`verifying`), RR-3
//! (the Phase 4 part) and "The kinds of job" (`task_check`: pure, keyed by
//! the tip of the task branch and the hash of the check).
//!
//! - [`plan`] decides from facts alone when it can: `not_applicable` only
//!   for no commits or `**Check:** none` (F2), and `failed` with `no check`
//!   for a task with no check and no run default check (F3). Else the
//!   check runs as a `task_check` job keyed by the tip and the hash of the
//!   check ([`input_key`]).
//! - A verdict stands only for the tip that it judged (F5).
//! - [`Verified`] is the only value that moves a task to `done`. Only this
//!   module makes it, from a verdict that did not fail.

/// What the verifier decided on 1 attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The check passed.
    Passed,
    /// The check failed, or the task has no check.
    Failed,
    /// The facts need no check: no commits, or `**Check:** none`.
    NotApplicable,
}

impl Decision {
    pub const ALL: [Decision; 3] = [Decision::Passed, Decision::Failed, Decision::NotApplicable];

    /// The word of `verdict-<n>.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Passed => "passed",
            Decision::Failed => "failed",
            Decision::NotApplicable => "not_applicable",
        }
    }

    /// The decision of a word.
    pub fn parse(word: &str) -> Option<Decision> {
        Decision::ALL.into_iter().find(|d| d.as_str() == word)
    }
}

/// The step after a verdict that did not fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// A code kind: the review steps (ticket 20).
    Review,
    /// A judgment kind or a judge task: the structural check (ticket 21).
    Structural,
}

/// The kinds that get the structural check in place of the review steps.
const STRUCTURAL_KINDS: [&str; 6] = ["research", "design", "docs", "report", "creative", "judge"];

/// The step after the check of a task of the ticket kind `work_kind`.
pub fn next_step(work_kind: Option<&str>) -> Next {
    if work_kind.is_some_and(|k| STRUCTURAL_KINDS.contains(&k)) {
        Next::Structural
    } else {
        Next::Review
    }
}

/// The facts of 1 attempt of a task in `verifying`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts<'a> {
    pub task_id: &'a str,
    pub attempt: &'a str,
    /// The tip of the task branch.
    pub tip: &'a str,
    /// The commit that the worktree of the task started from, when it has
    /// one.
    pub base: Option<&'a str>,
    /// The check command: the task's own, else the run's default.
    pub check: Option<&'a str>,
    /// The hash of `check`, `none` with no check.
    pub check_hash: &'a str,
    /// The ticket says `**Check:** none`.
    pub check_none: bool,
    /// The effective kind of the task: `judge` for a judge task, else the
    /// ticket kind or the kind of its plan.
    pub kind: Option<&'a str>,
}

/// The input key of a `task_check` job: the tip and the hash of the check.
pub fn input_key(tip: &str, check_hash: &str) -> String {
    format!("{tip}:{check_hash}")
}

/// What the verifier does for 1 attempt before a check job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// The facts decide. No job runs.
    Decided(Verdict),
    /// The check runs as a `task_check` job with this input key.
    Check { input_key: String },
}

/// The reason of the verdict of a judge task with no check command.
pub const JUDGE_NO_CHECK: &str = "a judge task is decided by its structural check and its verdict";

/// The plan for the attempt of `f`. A judge task has no check of its own
/// (FM-33): with commits and no check command, the facts decide
/// `not_applicable`, and its structural check and its verdict decide next.
pub fn plan(f: &Facts<'_>) -> Plan {
    let decided = |decision, reason: &str| Plan::Decided(Verdict::of(f, decision, None, reason));
    if f.base == Some(f.tip) {
        return decided(Decision::NotApplicable, "no commits");
    }
    if f.kind == Some("judge") && f.check.is_none() {
        return decided(Decision::NotApplicable, JUDGE_NO_CHECK);
    }
    match f.check {
        Some(_) => Plan::Check {
            input_key: input_key(f.tip, f.check_hash),
        },
        None if f.check_none => {
            decided(Decision::NotApplicable, "the ticket says `**Check:** none`")
        }
        None => decided(
            Decision::Failed,
            "no check: the task has no check command and the run has no default check; \
             give the task a check, give the run one, or write `**Check:** none` in the ticket",
        ),
    }
}

/// The verdict of the ended `task_check` run `job` on the attempt of `f`:
/// `passed` or `failed`, with `text` of the run.
pub fn from_check(f: &Facts<'_>, job: i64, passed: bool, text: &str) -> Verdict {
    let decision = if passed {
        Decision::Passed
    } else {
        Decision::Failed
    };
    Verdict::of(f, decision, Some(job), text)
}

/// 1 verdict, as `verdict-<n>.json` and its ledger row hold it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub task_id: String,
    pub attempt: String,
    /// The number of the verdict of its task: 1, 2, and so on. The ledger
    /// row gives it.
    pub n: u32,
    pub decision: Decision,
    /// The tip that the verdict judged.
    pub tip: String,
    pub check_hash: String,
    /// The `task_check` run that gave the verdict, when one ran.
    pub job: Option<i64>,
    pub reason: String,
}

impl Verdict {
    fn of(f: &Facts<'_>, decision: Decision, job: Option<i64>, reason: &str) -> Verdict {
        Verdict {
            task_id: f.task_id.into(),
            attempt: f.attempt.into(),
            n: 0,
            decision,
            tip: f.tip.into(),
            check_hash: f.check_hash.into(),
            job,
            reason: reason.into(),
        }
    }

    /// Whether this verdict judged the attempt of `f`, at its tip, with its
    /// check. A verdict of another tip is stale (F5).
    pub fn judged(&self, f: &Facts<'_>) -> bool {
        self.attempt == f.attempt && self.tip == f.tip && self.check_hash == f.check_hash
    }

    /// The value that moves the task to `done`, unless the verdict failed.
    pub fn verified(&self) -> Option<Verified> {
        (self.decision != Decision::Failed).then(|| Verified {
            task_id: self.task_id.clone(),
            attempt: self.attempt.clone(),
            n: self.n,
            decision: self.decision,
            tip: self.tip.clone(),
        })
    }
}

/// A verdict that did not fail: the only value that moves a task from
/// `verifying` to `done`. Only this module makes it.
///
/// `verified_verdict_has_no_constructor_outside_the_verification_module`:
///
/// ```compile_fail,E0451
/// use horch_core::fleetd::task_execution::domain::verification::{Decision, Verified};
///
/// let _ = Verified {
///     task_id: "T-1".into(),
///     attempt: "T-1-a1".into(),
///     n: 1,
///     decision: Decision::Passed,
///     tip: "abc".into(),
/// };
/// ```
///
/// The same path names the type, so the code above fails only on its
/// private fields:
///
/// ```
/// use horch_core::fleetd::task_execution::domain::verification::{Decision, Verified};
///
/// fn tip(v: &Verified) -> &str {
///     v.tip()
/// }
/// let _ = (tip, Decision::Passed);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    task_id: String,
    attempt: String,
    n: u32,
    decision: Decision,
    tip: String,
}

impl Verified {
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn attempt(&self) -> &str {
        &self.attempt
    }

    /// The number of its verdict.
    pub fn n(&self) -> u32 {
        self.n
    }

    pub fn decision(&self) -> Decision {
        self.decision
    }

    /// The tip that its verdict judged.
    pub fn tip(&self) -> &str {
        &self.tip
    }

    /// Whether this value still stands: `newest` is the newest verdict of
    /// its task and is its own verdict, and the tip of the task branch is
    /// still `tip` (F5).
    pub fn stands(&self, newest: &Verdict, tip: &str) -> bool {
        newest.task_id == self.task_id
            && newest.n == self.n
            && newest.attempt == self.attempt
            && newest.decision != Decision::Failed
            && newest.tip == self.tip
            && tip == self.tip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(check: Option<&'a str>, base: Option<&'a str>, check_none: bool) -> Facts<'a> {
        Facts {
            task_id: "T-1",
            attempt: "T-1-a1",
            tip: "tip1",
            base,
            check,
            check_hash: if check.is_some() { "h1" } else { "none" },
            check_none,
            kind: None,
        }
    }

    #[test]
    fn changed_judge_task_with_no_command_skips_command_check_in_plan() {
        let judge = Facts {
            kind: Some("judge"),
            ..facts(None, Some("base"), false)
        };
        let v = decided(plan(&judge));
        assert_eq!(v.decision, Decision::NotApplicable);
        assert_eq!(v.reason, JUDGE_NO_CHECK);
        assert!(v.verified().is_some());
        for kind in ["docs", "feature"] {
            let other = Facts {
                kind: Some(kind),
                ..facts(None, Some("base"), false)
            };
            let v = decided(plan(&other));
            assert_eq!(v.decision, Decision::Failed, "{kind}");
            assert!(v.reason.starts_with("no check"), "{kind}");
        }
    }

    fn decided(p: Plan) -> Verdict {
        match p {
            Plan::Decided(v) => v,
            Plan::Check { input_key } => panic!("a check job {input_key}"),
        }
    }

    #[test]
    fn a_check_runs_as_a_job_keyed_by_the_tip_and_the_hash_of_the_check() {
        let p = plan(&facts(Some("cargo test"), Some("base"), false));
        assert_eq!(
            p,
            Plan::Check {
                input_key: "tip1:h1".into()
            }
        );
    }

    #[test]
    fn not_applicable_comes_only_from_no_commits_or_check_none() {
        let no_commits = decided(plan(&facts(Some("cargo test"), Some("tip1"), false)));
        assert_eq!(no_commits.decision, Decision::NotApplicable);
        assert_eq!(no_commits.reason, "no commits");
        let none = decided(plan(&facts(None, None, true)));
        assert_eq!(none.decision, Decision::NotApplicable);
        assert!(none.reason.contains("Check:** none"));
    }

    #[test]
    fn no_check_and_no_check_none_fails_with_no_check() {
        let v = decided(plan(&facts(None, None, false)));
        assert_eq!(v.decision, Decision::Failed);
        assert!(v.reason.starts_with("no check"));
        assert_eq!(v.verified(), None);
    }

    #[test]
    fn a_verdict_stands_only_for_the_tip_it_judged() {
        let f = facts(Some("cargo test"), None, false);
        let mut v = from_check(&f, 7, true, "exit 0");
        v.n = 1;
        assert!(v.judged(&f));
        let newer = Facts { tip: "tip2", ..f };
        assert!(!v.judged(&newer));
        let other_check = Facts {
            check_hash: "h2",
            ..f
        };
        assert!(!v.judged(&other_check));
        let verified = v.verified().unwrap();
        assert!(verified.stands(&v, "tip1"));
        assert!(!verified.stands(&v, "tip2"));
        let mut later = v.clone();
        later.n = 2;
        assert!(!verified.stands(&later, "tip1"));
    }

    #[test]
    fn each_decision_has_1_word() {
        for d in Decision::ALL {
            assert_eq!(Decision::parse(d.as_str()), Some(d));
        }
        assert_eq!(Decision::parse("done"), None);
    }

    #[test]
    fn a_judgment_kind_gets_the_structural_check() {
        assert_eq!(next_step(Some("docs")), Next::Structural);
        assert_eq!(next_step(Some("judge")), Next::Structural);
        assert_eq!(next_step(Some("feature")), Next::Review);
        assert_eq!(next_step(None), Next::Review);
    }
}
