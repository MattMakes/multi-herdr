//! The rules of the cancel and the requeue of 1 task by the operator
//! (`docs/specs/fleetd/design.md`, "The task state machine" and "A
//! cancel"). Each rule reads the stored word of `tasks.status`.
//!
//! - A cancel works from each state that is not terminal. With a live
//!   attempt, the stop of its process comes first, and the task is
//!   `cancelled` only after the process is gone ([`cancel_path`],
//!   [`stop_seen`]).
//! - A requeue: `escalated` -> `ready` with the attempt count at 0, and
//!   `failed` -> `ready` for its 1 replan ([`requeue_path`]).
//! - A cancel never leaves a judgment task with no judge task: the cancel
//!   of a judge task alone waits for the cancel of the task it judges
//!   ([`judge_cancel_allowed`]).

use super::task::{Facts, Recorded, Seen};

/// The cause of the end of an attempt that a cancel stops.
pub const TASK_CANCELLED: &str = "task_cancelled";

/// The terminal task states: no exit, also not to `cancelled`.
const TERMINAL: [&str; 2] = ["done", "cancelled"];

/// Whether `status` is a terminal task state, with no exit to `cancelled`.
pub fn terminal(status: &str) -> bool {
    TERMINAL.contains(&status)
}

/// The task state whose old attempt can still have a live process.
const STALLED: &str = "stalled";

/// How the cancel of 1 task goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelPath {
    /// `done` or `cancelled`: the door refuses the move to `cancelled`.
    Refused,
    /// No live attempt: the task moves to `cancelled` at once.
    Now,
    /// A live attempt, or a cancel that already waits for its stop: the
    /// stop comes first, then the move to `cancelled`.
    StopFirst,
}

/// The cancel of a task in the state `status`. `live_attempt`: an attempt
/// of the task holds its worker. `requested`: a cancel of the task waits
/// for the stop of its attempt. A `stalled` task waits for the stop of its
/// old attempt (FM-14), so its cancel also waits for that stop.
pub fn cancel_path(status: &str, live_attempt: bool, requested: bool) -> CancelPath {
    if terminal(status) {
        CancelPath::Refused
    } else if live_attempt || requested || status == STALLED {
        CancelPath::StopFirst
    } else {
        CancelPath::Now
    }
}

/// Whether a judge task alone may be cancelled while the task it judges is
/// in the state `judged_status`. Only a `cancelled` judged task lets its
/// judge go: the cancel of the judged task cancels its judge task.
pub fn judge_cancel_allowed(judged_status: &str) -> bool {
    judged_status == "cancelled"
}

/// How the requeue of 1 task by the operator goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequeuePath {
    /// `escalated` -> `ready`, with the attempt count at 0 (RR-6, row L8).
    Escalated,
    /// `failed` with no replan -> `ready`, with the attempt count at 0: its
    /// 1 replan.
    Replan,
    /// `failed` after its 1 replan: refused. The task escalates.
    ReplanUsed,
    /// `cancelled` is terminal: the door refuses the move to `ready`.
    Door,
    /// Each other state: refused, as the task is not held.
    NotHeld,
}

/// The requeue of a task in the state `status` with `replans` replans.
pub fn requeue_path(status: &str, replans: i64) -> RequeuePath {
    match status {
        "escalated" => RequeuePath::Escalated,
        "failed" if replans == 0 => RequeuePath::Replan,
        "failed" => RequeuePath::ReplanUsed,
        "cancelled" => RequeuePath::Door,
        _ => RequeuePath::NotHeld,
    }
}

/// Whether the stop of a cancel is seen: each process in `watch`, the
/// processes of the attempts of the task, is dead in `facts`. A process
/// that the pass did not read is not dead.
pub fn stop_seen(watch: &[Recorded], facts: &Facts) -> bool {
    watch
        .iter()
        .all(|p| facts.processes.get(p) == Some(&Seen::Dead))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test of each state of the 1 list of states is in
    /// `application::cancel`: this module names nothing outside its
    /// domain.
    #[test]
    fn a_state_that_is_not_terminal_has_a_cancel() {
        for s in ["pending", "running", "escalated"] {
            assert_eq!(cancel_path(s, false, false), CancelPath::Now, "{s}");
            assert_eq!(cancel_path(s, true, false), CancelPath::StopFirst, "{s}");
            assert_eq!(cancel_path(s, false, true), CancelPath::StopFirst, "{s}");
        }
        for s in TERMINAL {
            assert_eq!(cancel_path(s, true, true), CancelPath::Refused, "{s}");
        }
        // A stalled task has no live attempt, but its old process can live.
        assert_eq!(cancel_path("stalled", false, false), CancelPath::StopFirst);
    }

    #[test]
    fn a_judge_task_alone_is_cancelled_only_after_its_judged_task() {
        assert!(judge_cancel_allowed("cancelled"));
        for s in [
            "pending",
            "ready",
            "running",
            "stalled",
            "verifying",
            "escalated",
            "failed",
            "done",
        ] {
            assert!(!judge_cancel_allowed(s), "{s}");
        }
    }

    #[test]
    fn only_a_held_task_is_requeued() {
        assert_eq!(requeue_path("escalated", 1), RequeuePath::Escalated);
        assert_eq!(requeue_path("failed", 0), RequeuePath::Replan);
        assert_eq!(requeue_path("failed", 1), RequeuePath::ReplanUsed);
        assert_eq!(requeue_path("cancelled", 0), RequeuePath::Door);
        for s in ["pending", "ready", "running", "verifying", "done"] {
            assert_eq!(requeue_path(s, 0), RequeuePath::NotHeld, "{s}");
        }
    }

    #[test]
    fn the_stop_is_seen_only_when_each_process_is_dead() {
        let a = Recorded {
            pid: 10,
            pid_start: Some(1),
        };
        let b = Recorded {
            pid: 11,
            pid_start: None,
        };
        let mut facts = Facts::default();
        assert!(stop_seen(&[], &facts));
        assert!(!stop_seen(&[a], &facts), "a process not read is not dead");
        facts.processes.insert(a, Seen::Dead);
        facts.processes.insert(b, Seen::Alive);
        assert!(stop_seen(&[a], &facts));
        assert!(!stop_seen(&[a, b], &facts));
        facts.processes.insert(b, Seen::Unknown);
        assert!(!stop_seen(&[a, b], &facts));
    }
}
