#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Rejected,
    Crashed,
    TimedOut,
    Stalled,
    Killed,
    NoResult,
    Asked,
    Quota,
    CriticalOpen,
    ContextLimit,
    ResumeRefused,
}

impl Outcome {
    pub const ALL: [Outcome; 11] = [
        Outcome::Rejected,
        Outcome::Crashed,
        Outcome::TimedOut,
        Outcome::Stalled,
        Outcome::Killed,
        Outcome::NoResult,
        Outcome::Asked,
        Outcome::Quota,
        Outcome::CriticalOpen,
        Outcome::ContextLimit,
        Outcome::ResumeRefused,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Rejected => "rejected",
            Outcome::Crashed => "crashed",
            Outcome::TimedOut => "timed_out",
            Outcome::Stalled => "stalled",
            Outcome::Killed => "killed",
            Outcome::NoResult => "no_result",
            Outcome::Asked => "asked",
            Outcome::Quota => "quota",
            Outcome::CriticalOpen => "critical_open",
            Outcome::ContextLimit => "context_limit",
            Outcome::ResumeRefused => "resume_refused",
        }
    }

    pub fn burns_attempt(self) -> bool {
        !matches!(
            self,
            Outcome::Asked
                | Outcome::Quota
                | Outcome::CriticalOpen
                | Outcome::ContextLimit
                | Outcome::ResumeRefused
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Numbers {
    pub used: i64,
    pub limit: i64,
    pub replans: i64,
}

pub fn limit(profile_max: Option<u32>, run_max_attempts: i64) -> i64 {
    profile_max.map_or(run_max_attempts, i64::from)
}

pub fn backoff_s(backoff: &[u64], used: i64) -> u64 {
    let n = usize::try_from(used.saturating_sub(1)).unwrap_or(0);
    backoff
        .get(n)
        .or_else(|| backoff.last())
        .copied()
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rung6 {
    Retry { not_before: i64 },
    Fail { due_at: i64 },
    Escalate,
}

pub fn rung6(n: &Numbers, backoff: &[u64], replan_limit_s: u64, clock: i64) -> Rung6 {
    if n.used < n.limit {
        let wait = i64::try_from(backoff_s(backoff, n.used)).unwrap_or(i64::MAX);
        Rung6::Retry {
            not_before: clock.saturating_add(wait),
        }
    } else if n.replans >= 1 {
        Rung6::Escalate
    } else {
        let limit = i64::try_from(replan_limit_s).unwrap_or(i64::MAX);
        Rung6::Fail {
            due_at: clock.saturating_add(limit),
        }
    }
}

pub fn replan_overdue(due_at: Option<i64>, clock: i64) -> bool {
    due_at.is_some_and(|d| clock >= d)
}

pub fn claimable_at(not_before: Option<i64>, clock: i64) -> bool {
    not_before.is_none_or(|t| clock >= t)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffFacts<'a> {
    pub attempt: &'a str,
    pub why: &'a str,
    pub progress: &'a str,
    pub diff: &'a str,
    pub transcript_tail: &'a str,
}

pub fn handoff_text(f: &HandoffFacts<'_>) -> String {
    let section = |title: &str, body: &str| {
        let body = if body.trim().is_empty() {
            "(empty)\n".to_string()
        } else if body.ends_with('\n') {
            body.to_string()
        } else {
            format!("{body}\n")
        };
        format!("\n## {title}\n\n~~~~\n{body}~~~~\n")
    };
    format!(
        "# Handoff of attempt {}\n\n{}\n\nThe worktree keeps the work of that attempt. \
         Read this file, then go on from the last step.\n{}{}{}",
        f.attempt,
        f.why,
        section("Progress (outbox/progress.jsonl)", f.progress),
        section("git diff", f.diff),
        section("Transcript tail (transcript.jsonl)", f.transcript_tail),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKOFF_S: [u64; 3] = [30, 120, 480];
    const REPLAN_LIMIT_S: u64 = 1800;

    fn at(used: i64, limit: i64, replans: i64, clock: i64) -> Rung6 {
        let n = Numbers {
            used,
            limit,
            replans,
        };
        rung6(&n, &BACKOFF_S, REPLAN_LIMIT_S, clock)
    }

    fn retry_at(s: i64) -> Rung6 {
        Rung6::Retry { not_before: s }
    }

    #[test]
    fn retry_backoff_30s_2m_8m_then_replan_then_escalate() {
        let limit = limit(Some(4), 3);
        assert_eq!(limit, 4, "the profile limit wins over the run's");
        assert_eq!(at(1, limit, 0, 1000), retry_at(1030));
        assert_eq!(at(2, limit, 0, 1000), retry_at(1120));
        assert_eq!(at(3, limit, 0, 1000), retry_at(1480));
        assert_eq!(
            at(4, limit, 0, 1000),
            Rung6::Fail {
                due_at: 1000 + 1800
            }
        );
        assert_eq!(at(1, limit, 1, 5000), retry_at(5030));
        assert_eq!(at(4, limit, 1, 5000), Rung6::Escalate);
    }

    #[test]
    fn the_11_stored_outcome_words_stay() {
        let words: Vec<&str> = Outcome::ALL.into_iter().map(Outcome::as_str).collect();
        assert_eq!(
            words,
            [
                "rejected",
                "crashed",
                "timed_out",
                "stalled",
                "killed",
                "no_result",
                "asked",
                "quota",
                "critical_open",
                "context_limit",
                "resume_refused",
            ]
        );
    }

    #[test]
    fn asked_quota_and_critical_open_burn_no_attempt() {
        let burning: Vec<Outcome> = Outcome::ALL
            .into_iter()
            .filter(|o| o.burns_attempt())
            .collect();
        assert_eq!(
            burning,
            [
                Outcome::Rejected,
                Outcome::Crashed,
                Outcome::TimedOut,
                Outcome::Stalled,
                Outcome::Killed,
                Outcome::NoResult,
            ]
        );
        for o in [
            Outcome::Asked,
            Outcome::Quota,
            Outcome::CriticalOpen,
            Outcome::ContextLimit,
            Outcome::ResumeRefused,
        ] {
            assert!(!o.burns_attempt(), "{o:?}");
        }
    }

    #[test]
    fn handoff_text_holds_progress_diff_and_transcript_tail() {
        let text = handoff_text(&HandoffFacts {
            attempt: "T-1/A1",
            why: "The context reached the limit.",
            progress: "{\"text\":\"step 1\"}\n",
            diff: "+new line",
            transcript_tail: "",
        });
        assert!(text.starts_with("# Handoff of attempt T-1/A1\n"));
        assert!(text.contains("The context reached the limit."));
        assert!(text.contains("~~~~\n{\"text\":\"step 1\"}\n~~~~"));
        assert!(text.contains("~~~~\n+new line\n~~~~"));
        assert!(text.contains("## Transcript tail (transcript.jsonl)\n\n~~~~\n(empty)\n~~~~"));
    }

    #[test]
    fn last_backoff_value_repeats() {
        let b = [30, 120, 480];
        assert_eq!(backoff_s(&b, 1), 30);
        assert_eq!(backoff_s(&b, 3), 480);
        assert_eq!(backoff_s(&b, 4), 480);
        assert_eq!(backoff_s(&b, 9), 480);
        assert_eq!(at(7, 10, 0, 0), retry_at(480));
    }

    #[test]
    fn limit_counts_attempts_since_the_attempt_floor() {
        let limit = limit(None, 3);
        assert_eq!(limit, 3);
        let (attempts, attempt_floor) = (5, 3);
        assert_eq!(at(attempts - attempt_floor, limit, 0, 0), retry_at(120));
        let (attempts, attempt_floor) = (6, 3);
        assert!(matches!(
            at(attempts - attempt_floor, limit, 0, 0),
            Rung6::Fail { .. }
        ));
    }

    #[test]
    fn claim_and_overdue_read_the_fleet_clock() {
        assert!(claimable_at(None, 0));
        assert!(!claimable_at(Some(130), 129));
        assert!(claimable_at(Some(130), 130));
        assert!(!replan_overdue(None, 9999));
        assert!(!replan_overdue(Some(1800), 1799));
        assert!(replan_overdue(Some(1800), 1800));
    }
}
