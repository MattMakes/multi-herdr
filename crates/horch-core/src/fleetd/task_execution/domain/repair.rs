//! The repair pass and the failure limit of a repair attempt (tickets 15 and 34).
//!
//! The repair pass is FM-6, the outside end: when the agent process of a
//! push attempt exits with commits since its base, and with no
//! `result.json` and no question file, the wrapper (`horch worker`) runs 1
//! short resume of the same session with [`REPAIR_LINE`], then exits. The
//! repair is 1 event on the attempt, written before the resume, so 1
//! attempt never gets a second repair. The repair burns no attempt.
//!
//! The failure limit is the rule of a resolver or fix attempt of a `done`
//! task (`docs/specs/fleetd/design.md`, "Landing a task branch on its
//! feature branch", RR-6): each failed attempt adds 1 to `repair_attempts`
//! of its task branch, and [`next_repair`] says when the next attempt
//! starts. At `max_attempts` of the task, no attempt starts, and the branch
//! has 1 item on the needs-you list.

use super::retry;

/// The fixed line of the repair resume.
pub const REPAIR_LINE: &str = "Write the result of your task now: run `horch result`.";

/// The cause of the 1 event on the attempt that records its repair.
pub const REPAIR_CAUSE: &str = "repair pass";

/// What the wrapper knows about 1 push attempt when its agent ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RepairFacts {
    /// The agent process exited.
    pub exited: bool,
    /// The branch of the attempt has commits since its base.
    pub commits_since_base: bool,
    /// `outbox/result.json` of the attempt exists.
    pub result_json: bool,
    /// `outbox/questions/` of the attempt holds a question file.
    pub question_file: bool,
    /// The attempt has the event of an earlier repair.
    pub repaired: bool,
}

/// The attempt gets its 1 repair: the agent exited with commits, with no
/// result and no question file, and no earlier repair.
pub fn needs_repair(f: &RepairFacts) -> bool {
    f.exited && f.commits_since_base && !f.result_json && !f.question_file && !f.repaired
}

/// What the repair pass reads and does for 1 attempt.
pub trait RepairPort {
    type Error;
    /// The facts of the attempt now.
    fn facts(&mut self) -> Result<RepairFacts, Self::Error>;
    /// Write the 1 event of the repair on the attempt.
    fn record(&mut self) -> Result<(), Self::Error>;
    /// Resume the session of the agent with `line`, and wait for it. `None`:
    /// a signal ended it.
    fn resume(&mut self, line: &str) -> Result<Option<i32>, Self::Error>;
}

/// What [`repair_pass`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repaired {
    /// The attempt needs no repair.
    No,
    /// The resume ran, with the exit code of the resumed agent.
    Resumed(Option<i32>),
}

/// The repair pass of 1 attempt: when [`needs_repair`], record the repair,
/// then resume the session with [`REPAIR_LINE`]. The record comes first,
/// so a crash during the resume gives no second repair.
pub fn repair_pass<P: RepairPort>(port: &mut P) -> Result<Repaired, P::Error> {
    if !needs_repair(&port.facts()?) {
        return Ok(Repaired::No);
    }
    port.record()?;
    Ok(Repaired::Resumed(port.resume(REPAIR_LINE)?))
}

/// The kind of the needs-you item of a task branch at its repair limit.
/// Its key is `<kind>:<branch>`.
pub const REPAIR_LIMIT_KIND: &str = "repair_limit";

/// The repair count of 1 task branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepairCount {
    /// `repair_attempts` of the branch.
    pub failed: i64,
    /// `max_attempts` of the task.
    pub max_attempts: i64,
    /// The fleet clock of the last counted failure of the current need of
    /// repair; `None` when no attempt of it failed.
    pub failed_at: Option<i64>,
}

/// What the repair pass does for 1 branch that waits for a repair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextRepair {
    /// Start the next attempt now.
    Start,
    /// The backoff holds the next attempt until this fleet clock.
    Wait { not_before: i64 },
    /// The branch is at the limit: no attempt, 1 needs-you item.
    Limit,
}

/// The next repair of a branch at fleet clock `clock`. After `failed`
/// failures, the next attempt waits `backoff_s[failed - 1]` seconds from
/// the last failure (the backoff of rung 6). A count of 0, after
/// `horch task requeue`, waits for nothing.
pub fn next_repair(n: &RepairCount, backoff_s: &[u64], clock: i64) -> NextRepair {
    if n.failed >= n.max_attempts {
        return NextRepair::Limit;
    }
    let wait = if n.failed > 0 {
        retry::backoff_s(backoff_s, n.failed)
    } else {
        0
    };
    let wait = i64::try_from(wait).unwrap_or(i64::MAX);
    match n.failed_at.map(|at| at.saturating_add(wait)) {
        Some(not_before) if clock < not_before => NextRepair::Wait { not_before },
        _ => NextRepair::Start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKOFF_S: [u64; 3] = [30, 120, 480];

    fn count(failed: i64, failed_at: Option<i64>) -> RepairCount {
        RepairCount {
            failed,
            max_attempts: 4,
            failed_at,
        }
    }

    #[test]
    fn first_need_of_repair_starts_at_once() {
        assert_eq!(
            next_repair(&count(0, None), &BACKOFF_S, 0),
            NextRepair::Start
        );
    }

    #[test]
    fn each_failure_waits_its_backoff_from_the_failure() {
        let at = |failed, clock| next_repair(&count(failed, Some(100)), &BACKOFF_S, clock);
        assert_eq!(at(1, 129), NextRepair::Wait { not_before: 130 });
        assert_eq!(at(1, 130), NextRepair::Start);
        assert_eq!(at(2, 219), NextRepair::Wait { not_before: 220 });
        assert_eq!(at(3, 580), NextRepair::Start);
    }

    #[test]
    fn at_max_attempts_no_attempt_starts() {
        assert_eq!(
            next_repair(&count(4, Some(0)), &BACKOFF_S, 1_000_000),
            NextRepair::Limit
        );
        let past = RepairCount {
            max_attempts: 2,
            ..count(3, None)
        };
        assert_eq!(next_repair(&past, &BACKOFF_S, 0), NextRepair::Limit);
    }

    #[test]
    fn count_at_0_after_a_requeue_waits_for_nothing() {
        assert_eq!(
            next_repair(&count(0, Some(100)), &BACKOFF_S, 100),
            NextRepair::Start
        );
    }

    fn due() -> RepairFacts {
        RepairFacts {
            exited: true,
            commits_since_base: true,
            ..RepairFacts::default()
        }
    }

    #[test]
    fn exit_with_commits_and_no_result_needs_a_repair() {
        assert!(needs_repair(&due()));
    }

    #[test]
    fn each_other_fact_stops_the_repair() {
        let stops = [
            RepairFacts {
                exited: false,
                ..due()
            },
            RepairFacts {
                commits_since_base: false,
                ..due()
            },
            RepairFacts {
                result_json: true,
                ..due()
            },
            RepairFacts {
                question_file: true,
                ..due()
            },
            RepairFacts {
                repaired: true,
                ..due()
            },
        ];
        for f in stops {
            assert!(!needs_repair(&f), "{f:?}");
        }
    }

    /// A port that only counts its calls.
    struct Counting {
        facts: RepairFacts,
        calls: Vec<String>,
    }

    impl RepairPort for Counting {
        type Error = String;
        fn facts(&mut self) -> Result<RepairFacts, String> {
            self.calls.push("facts".into());
            Ok(self.facts)
        }
        fn record(&mut self) -> Result<(), String> {
            self.calls.push("record".into());
            Ok(())
        }
        fn resume(&mut self, line: &str) -> Result<Option<i32>, String> {
            self.calls.push(format!("resume {line}"));
            Ok(Some(0))
        }
    }

    #[test]
    fn the_record_comes_before_the_resume() {
        let mut port = Counting {
            facts: due(),
            calls: Vec::new(),
        };
        assert_eq!(repair_pass(&mut port), Ok(Repaired::Resumed(Some(0))));
        assert_eq!(
            port.calls,
            ["facts", "record", &format!("resume {REPAIR_LINE}")]
        );
    }

    #[test]
    fn no_repair_writes_nothing_and_resumes_nothing() {
        let mut port = Counting {
            facts: RepairFacts::default(),
            calls: Vec::new(),
        };
        assert_eq!(repair_pass(&mut port), Ok(Repaired::No));
        assert_eq!(port.calls, ["facts"]);
    }

    #[test]
    fn a_failed_record_runs_no_resume() {
        struct NoRecord;
        impl RepairPort for NoRecord {
            type Error = &'static str;
            fn facts(&mut self) -> Result<RepairFacts, &'static str> {
                Ok(due())
            }
            fn record(&mut self) -> Result<(), &'static str> {
                Err("the ledger is locked")
            }
            fn resume(&mut self, _: &str) -> Result<Option<i32>, &'static str> {
                panic!("a repair with no record must not resume");
            }
        }
        assert_eq!(repair_pass(&mut NoRecord), Err("the ledger is locked"));
    }
}
