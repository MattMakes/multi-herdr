use std::fmt::Display;

use super::model::{TaskStatus, WorkerStatus, WorktreeStatus};

pub const TASKS_IN_FLIGHT: &[TaskStatus] = &[TaskStatus::Running, TaskStatus::Verifying];

pub const TASKS_WAITING: &[TaskStatus] = &[TaskStatus::Pending, TaskStatus::Ready];

pub const TASKS_QUEUED: &[TaskStatus] = &[
    TaskStatus::Pending,
    TaskStatus::Ready,
    TaskStatus::Escalated,
];

pub const TASKS_ENDED: &[TaskStatus] = &[TaskStatus::Done, TaskStatus::Cancelled];

pub const WORKERS_LIVE: &[WorkerStatus] = &[
    WorkerStatus::Spawning,
    WorkerStatus::Starting,
    WorkerStatus::Claiming,
    WorkerStatus::Working,
];

pub const WORKERS_UNCLAIMED: &[WorkerStatus] = &[WorkerStatus::Spawning, WorkerStatus::Starting];

pub const WORKTREES_OPEN: &[WorktreeStatus] = &[WorktreeStatus::Open, WorktreeStatus::Landing];

pub const WORKTREES_HELD: &[WorktreeStatus] = &[
    WorktreeStatus::Open,
    WorktreeStatus::Landing,
    WorktreeStatus::Landed,
    WorktreeStatus::CleanupFailed,
];

pub const WORKTREES_LANDED: &[WorktreeStatus] =
    &[WorktreeStatus::Landed, WorktreeStatus::CleanupFailed];

pub const WORKTREES_GONE: &[WorktreeStatus] = &[WorktreeStatus::Cleaned, WorktreeStatus::Abandoned];

pub fn marks(first: usize, n: usize) -> String {
    (first..first + n)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn word(status: impl Display) -> String {
    format!("'{status}'")
}

pub fn list<T: Display>(set: &[T]) -> String {
    set.iter().map(word).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_quotes_each_word_in_order() {
        assert_eq!(list(WORKTREES_OPEN), "'open', 'landing'");
        assert_eq!(word(TaskStatus::Ready), "'ready'");
    }

    #[test]
    fn live_workers_match_is_live() {
        for s in WorkerStatus::ALL {
            assert_eq!(WORKERS_LIVE.contains(s), s.is_live(), "{s}");
        }
    }
}
