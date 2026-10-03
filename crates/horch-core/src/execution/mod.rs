//! Executions: one run of one worker, orchestrator, candidate or judge.
//!
//! [`model`] holds the vocabulary (A1). [`legacy`] is the on-disk ledger
//! record and [`store`] reads and writes it (A6); planning and the service
//! follow later in A6.

pub mod legacy;
pub mod model;
pub mod store;

pub use model::{
    ExecutionKind, ExecutionStatus, FailureKind, LaunchStage, SessionMode, SessionState, TilingMode,
};
