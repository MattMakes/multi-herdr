#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Passed,
    Failed,
    NotApplicable,
}

impl Decision {
    pub const ALL: [Decision; 3] = [Decision::Passed, Decision::Failed, Decision::NotApplicable];

    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Passed => "passed",
            Decision::Failed => "failed",
            Decision::NotApplicable => "not_applicable",
        }
    }

    pub fn parse(word: &str) -> Option<Decision> {
        Decision::ALL.into_iter().find(|d| d.as_str() == word)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Review,
    Structural,
}

const STRUCTURAL_KINDS: [&str; 6] = ["research", "design", "docs", "report", "creative", "judge"];

pub fn next_step(work_kind: Option<&str>) -> Next {
    if work_kind.is_some_and(|k| STRUCTURAL_KINDS.contains(&k)) {
        Next::Structural
    } else {
        Next::Review
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts<'a> {
    pub task_id: &'a str,
    pub attempt: &'a str,
    pub tip: &'a str,
    pub base: Option<&'a str>,
    pub check: Option<&'a str>,
    pub check_hash: &'a str,
    pub check_none: bool,
    pub kind: Option<&'a str>,
}

pub fn input_key(tip: &str, check_hash: &str) -> String {
    format!("{tip}:{check_hash}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    Decided(Verdict),
    Check { input_key: String },
}

pub const JUDGE_NO_CHECK: &str = "a judge task is decided by its structural check and its verdict";

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

pub fn from_check(f: &Facts<'_>, job: i64, passed: bool, text: &str) -> Verdict {
    let decision = if passed {
        Decision::Passed
    } else {
        Decision::Failed
    };
    Verdict::of(f, decision, Some(job), text)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub task_id: String,
    pub attempt: String,
    pub n: u32,
    pub decision: Decision,
    pub tip: String,
    pub check_hash: String,
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

    pub fn judged(&self, f: &Facts<'_>) -> bool {
        self.attempt == f.attempt && self.tip == f.tip && self.check_hash == f.check_hash
    }

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

    pub fn n(&self) -> u32 {
        self.n
    }

    pub fn decision(&self) -> Decision {
        self.decision
    }

    pub fn tip(&self) -> &str {
        &self.tip
    }

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
