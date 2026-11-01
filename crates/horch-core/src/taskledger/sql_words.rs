//! Status words for SQL text, built from the enums in [`super::model`].
//!
//! A query never spells a status word: `format!` puts [`word`] or [`list`]
//! in, so a word added to an enum reaches every reader (#240). A misspelled
//! literal is valid SQL that matches 0 rows; a missing enum variant does not
//! compile.

use std::fmt::Display;

use super::model::{TaskStatus, WorkerStatus, WorktreeStatus};

/// Tasks a worker is on: running, or past its last step into the land.
pub const TASKS_IN_FLIGHT: &[TaskStatus] = &[TaskStatus::Running, TaskStatus::Landing];

/// Tasks that are over.
pub const TASKS_ENDED: &[TaskStatus] = &[TaskStatus::Done, TaskStatus::Cancelled];

/// Workers that may still run (the same set as [`WorkerStatus::is_live`]).
pub const WORKERS_LIVE: &[WorkerStatus] = &[
    WorkerStatus::Spawning,
    WorkerStatus::Starting,
    WorkerStatus::Claiming,
    WorkerStatus::Working,
];

/// Workers that have not claimed a task yet.
pub const WORKERS_UNCLAIMED: &[WorkerStatus] = &[WorkerStatus::Spawning, WorkerStatus::Starting];

/// Worktrees whose work is not on the run branch yet.
pub const WORKTREES_OPEN: &[WorktreeStatus] = &[WorktreeStatus::Open, WorktreeStatus::Landing];

/// Worktrees whose folder or work a task still holds.
pub const WORKTREES_HELD: &[WorktreeStatus] = &[
    WorktreeStatus::Open,
    WorktreeStatus::Landing,
    WorktreeStatus::Landed,
    WorktreeStatus::CleanupFailed,
];

/// Worktrees whose work is on the run branch, folder not yet removed.
pub const WORKTREES_LANDED: &[WorktreeStatus] =
    &[WorktreeStatus::Landed, WorktreeStatus::CleanupFailed];

/// Worktrees with nothing left to do: the folder is gone.
pub const WORKTREES_GONE: &[WorktreeStatus] = &[WorktreeStatus::Cleaned, WorktreeStatus::Abandoned];

/// `'word'`: one status word as SQL text.
pub fn word(status: impl Display) -> String {
    format!("'{status}'")
}

/// `'a', 'b'`: the words of `set` for an `IN (...)` list.
pub fn list<T: Display>(set: &[T]) -> String {
    set.iter().map(word).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_quotes_each_word_in_order() {
        assert_eq!(list(WORKTREES_OPEN), "'open', 'landing'");
        assert_eq!(word(TaskStatus::Queued), "'queued'");
    }

    #[test]
    fn live_workers_match_is_live() {
        for s in WorkerStatus::ALL {
            assert_eq!(WORKERS_LIVE.contains(s), s.is_live(), "{s}");
        }
    }
}
