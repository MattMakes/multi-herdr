//! Executions: one run of one worker, orchestrator, candidate or judge.
//!
//! [`model`] holds the vocabulary (A1) and the spawn types (A6). [`legacy`]
//! is the on-disk ledger record, [`store`] reads and writes it, and
//! [`records`] holds the ledger's lifecycle rules. [`plan`]
//! turns a spawn request into a plan with no I/O, and [`service`] applies
//! it. [`lifecycle`] holds the worker's run and `done`.

pub mod legacy;
pub mod lifecycle;
pub mod model;
pub mod plan;
pub mod records;
pub mod service;
pub mod store;

pub use lifecycle::ReportTarget;
pub use model::{
    Execution, ExecutionKind, ExecutionPlan, ExecutionStatus, FailureKind, LaunchPlan, LaunchStage,
    SessionMode, SessionState, SpawnRequest, Task, TilingMode, WorkspacePlan,
};
