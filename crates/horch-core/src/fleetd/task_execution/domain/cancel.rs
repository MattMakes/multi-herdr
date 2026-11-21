use super::task::{Facts, Recorded, Seen};

pub const TASK_CANCELLED: &str = "task_cancelled";

const TERMINAL: [&str; 2] = ["done", "cancelled"];

pub fn terminal(status: &str) -> bool {
    TERMINAL.contains(&status)
}

const STALLED: &str = "stalled";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelPath {
    Refused,
    Now,
    StopFirst,
}

pub fn cancel_path(status: &str, live_attempt: bool, requested: bool) -> CancelPath {
    if terminal(status) {
        CancelPath::Refused
    } else if live_attempt || requested || status == STALLED {
        CancelPath::StopFirst
    } else {
        CancelPath::Now
    }
}

pub fn judge_cancel_allowed(judged_status: &str) -> bool {
    judged_status == "cancelled"
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequeuePath {
    Escalated,
    Replan,
    ReplanUsed,
    Door,
    NotHeld,
}

pub fn requeue_path(status: &str, replans: i64) -> RequeuePath {
    match status {
        "escalated" => RequeuePath::Escalated,
        "failed" if replans == 0 => RequeuePath::Replan,
        "failed" => RequeuePath::ReplanUsed,
        "cancelled" => RequeuePath::Door,
        _ => RequeuePath::NotHeld,
    }
}

pub fn stop_seen(watch: &[Recorded], facts: &Facts) -> bool {
    watch
        .iter()
        .all(|p| facts.processes.get(p) == Some(&Seen::Dead))
}

#[cfg(test)]
mod tests {
    use super::*;

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
