//! The domain layer of task execution: rules with no I/O.

pub mod cancel;
pub mod lease;
pub mod quota;
pub mod retry;
pub mod stall;
pub mod task;
pub mod waits;
