//! The retry rule of rung 6: the attempt limit and the backoff
//! (`docs/specs/fleetd/design.md`, "Stall ladder", rung 6, and "The task
//! state machine", the rows `stalled`, `failed` and `escalated`).
//!
//! Each outcome that is not verified and is not `asked`, `quota` or
//! `critical_open` burns 1 attempt. With attempts left, the task waits for
//! its backoff on the fleet clock in `ready`. With no attempt left, it goes
//! to `failed` for its 1 replan, then to `escalated` (RR-6).

/// How an attempt ended, when the verifier did not verify it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The verifier rejected the result.
    Rejected,
    /// The process of the attempt died.
    Crashed,
    /// No first signal within the start timeout.
    TimedOut,
    /// Rungs 3 and 4 of the stall ladder.
    Stalled,
    /// The attempt was stopped.
    Killed,
    /// The process exited with no result.
    NoResult,
    /// The worker asked a question.
    Asked,
    /// A usage limit.
    Quota,
    /// The judge reports an open CRITICAL item.
    CriticalOpen,
    /// The context of the attempt reached the limit, and the attempt ended
    /// with a handoff (FM-20).
    ContextLimit,
}

impl Outcome {
    pub const ALL: [Outcome; 10] = [
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
    ];

    /// The word of the design, as `attempts.outcome` stores it.
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
        }
    }

    /// A question, a usage limit, an open CRITICAL item of a judge and the
    /// context limit burn no attempt. Each other outcome burns 1.
    pub fn burns_attempt(self) -> bool {
        !matches!(
            self,
            Outcome::Asked | Outcome::Quota | Outcome::CriticalOpen | Outcome::ContextLimit
        )
    }
}

/// The numbers of rung 6 for 1 task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Numbers {
    /// The attempts since the last operator requeue, the burned one
    /// included: `attempts - attempt_floor`.
    pub used: i64,
    /// The attempt limit: [`limit`].
    pub limit: i64,
    /// The replans of the task: 0 or 1.
    pub replans: i64,
}

/// The attempt limit of a task: `retry.max_attempts` of its profile
/// (`profile_max`), else the `max_attempts` of its run
/// (`docs/tickets/fleetd-p2/MAP.md`, Q10).
pub fn limit(profile_max: Option<u32>, run_max_attempts: i64) -> i64 {
    profile_max.map_or(run_max_attempts, i64::from)
}

/// The backoff after `used` attempts: `backoff_s[used - 1]`, and the last
/// value repeats. An empty list gives 0.
pub fn backoff_s(backoff: &[u64], used: i64) -> u64 {
    let n = usize::try_from(used.saturating_sub(1)).unwrap_or(0);
    backoff
        .get(n)
        .or_else(|| backoff.last())
        .copied()
        .unwrap_or(0)
}

/// Where rung 6 moves a task after 1 burned attempt. Each time is a value
/// of the fleet clock, in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rung6 {
    /// `ready`, first in line, not claimed before `not_before`.
    Retry { not_before: i64 },
    /// `failed`: the orchestrator of the run replans it before `due_at`.
    Fail { due_at: i64 },
    /// `failed`, then `escalated` at once: the 1 replan is used.
    Escalate,
}

/// Rung 6 at fleet clock `clock` for a task with `n`.
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

/// A `failed` task with `due_at` is past its replan limit at `clock`.
pub fn replan_overdue(due_at: Option<i64>, clock: i64) -> bool {
    due_at.is_some_and(|d| clock >= d)
}

/// A `ready` task with `not_before` can be claimed at `clock`.
pub fn claimable_at(not_before: Option<i64>, clock: i64) -> bool {
    not_before.is_none_or(|t| clock >= t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design default of `backoff_s`.
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
        // Attempts 1 to 3 fail: 30 s, 2 min, 8 min on the fleet clock.
        assert_eq!(at(1, limit, 0, 1000), retry_at(1030));
        assert_eq!(at(2, limit, 0, 1000), retry_at(1120));
        assert_eq!(at(3, limit, 0, 1000), retry_at(1480));
        // Attempt 4 fails: no attempt left, failed for its 1 replan.
        assert_eq!(
            at(4, limit, 0, 1000),
            Rung6::Fail {
                due_at: 1000 + 1800
            }
        );
        // The replan starts the count again from 0 (attempt_floor).
        assert_eq!(at(1, limit, 1, 5000), retry_at(5030));
        // The last attempt after the replan fails: escalated.
        assert_eq!(at(4, limit, 1, 5000), Rung6::Escalate);
    }

    #[test]
    fn the_10_stored_outcome_words_stay() {
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
        ] {
            assert!(!o.burns_attempt(), "{o:?}");
        }
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
        // Preservation 04 L4: the run's max_attempts with no profile limit,
        // counted from the attempt floor of the last operator requeue.
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
