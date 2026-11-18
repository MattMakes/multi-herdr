//! The outcome `context_limit` of an attempt, which burns no attempt, and
//! its loop guard (fleetd-p7 MAP.md Q2; `docs/specs/fleetd/design.md`,
//! FM-20 and FR-9).
//!
//! A CLI with no compaction setting (Antigravity) cannot hold its context
//! at 256k by itself. The wrapper counts the context tokens of its stream
//! and ends the attempt at [`CONTEXT_LIMIT`]. The task goes on at once in
//! the same worktree with a handoff file. The guard stops a loop: when 2
//! attempts in a row end with `context_limit` and the task branch has no
//! new commit between them, the task does not go to `ready` again.

use super::retry::Outcome;

/// The context limit of each agent, in tokens (FR-9). The wrapper applies
/// it only to a CLI with no compaction setting.
pub const CONTEXT_LIMIT: u64 = 256_000;

/// The context of `tokens` reached the limit.
pub fn reached(tokens: u64) -> bool {
    tokens >= CONTEXT_LIMIT
}

/// How 1 ended attempt of a task ended, as the guard reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End<'a> {
    /// The `attempts.outcome` word, `None` when the row has none.
    pub outcome: Option<&'a str>,
    /// The head commit of the worktree at the end of the attempt, `None`
    /// when fleetd did not record it.
    pub head: Option<&'a str>,
}

impl End<'_> {
    fn at_limit(&self) -> bool {
        self.outcome == Some(Outcome::ContextLimit.as_str())
    }
}

/// Whether the attempt `this`, which ended with `context_limit`, and the
/// attempt before it (`previous`) form a loop: both ended at the limit,
/// and the head of the worktree did not move. A guard whose fact is
/// unknown does not hold: a missing head is not a loop.
pub fn loops(previous: Option<&End<'_>>, this: &End<'_>) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if !previous.at_limit() || !this.at_limit() {
        return false;
    }
    matches!((previous.head, this.head), (Some(a), Some(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Option<&str> = Some("context_limit");

    fn end<'a>(outcome: Option<&'a str>, head: Option<&'a str>) -> End<'a> {
        End { outcome, head }
    }

    #[test]
    fn the_limit_is_256000_tokens() {
        assert_eq!(CONTEXT_LIMIT, 256_000);
        assert!(!reached(255_999));
        assert!(reached(256_000));
        assert!(reached(300_000));
    }

    #[test]
    fn two_limits_at_the_same_head_loop() {
        let this = end(LIMIT, Some("abc"));
        assert!(loops(Some(&end(LIMIT, Some("abc"))), &this));
    }

    #[test]
    fn a_new_commit_between_the_limits_is_no_loop() {
        let this = end(LIMIT, Some("def"));
        assert!(!loops(Some(&end(LIMIT, Some("abc"))), &this));
    }

    #[test]
    fn the_first_limit_is_no_loop() {
        assert!(!loops(None, &end(LIMIT, Some("abc"))));
        let crashed = end(Some("crashed"), Some("abc"));
        assert!(!loops(Some(&crashed), &end(LIMIT, Some("abc"))));
    }

    #[test]
    fn an_unknown_head_is_no_loop() {
        assert!(!loops(Some(&end(LIMIT, None)), &end(LIMIT, Some("abc"))));
        assert!(!loops(Some(&end(LIMIT, Some("abc"))), &end(LIMIT, None)));
        assert!(!loops(Some(&end(LIMIT, None)), &end(LIMIT, None)));
    }
}
