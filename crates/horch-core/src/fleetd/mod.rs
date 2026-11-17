//! fleetd: the fleet daemon, as 1 folder for each context of `ddd/08-code`.
//!
//! Phases 1 and 2 use 4 contexts, and Phase 3 adds `worktree_custody`. Each
//! has `domain/`, `application/` and `adapters/`.
//! `crates/horch-core/tests/arch_scan.rs` guards the layout.

pub mod fleet_runtime;
pub mod ideas;
pub mod task_execution;
pub mod worker_profiles;
pub mod worktree_custody;
