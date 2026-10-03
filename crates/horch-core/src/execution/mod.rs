//! Executions: one run of one worker, orchestrator, candidate or judge.
//!
//! Phase A1 holds only the vocabulary in [`model`]; planning, the service and
//! the store arrive in A6.

pub mod model;

pub use model::{
    ExecutionKind, ExecutionStatus, FailureKind, LaunchStage, SessionMode, SessionState, TilingMode,
};
