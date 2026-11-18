//! The domain layer of task execution: rules with no I/O.

pub mod cancel;
pub mod dispatch;
pub mod judge_pair;
pub mod lease;
pub mod outbox;
pub mod quota;
pub mod repair;
pub mod result;
pub mod retry;
pub mod stall;
pub mod structural;
pub mod task;
pub mod task_files;
pub mod verification;
pub mod waits;
