//! fleetd: the fleet daemon, as 1 folder for each context of `ddd/08-code`.
//!
//! Phase 1 uses 4 contexts. Each has `domain/`, `application/` and
//! `adapters/`. `crates/horch-core/tests/arch_scan.rs` guards the layout.

pub mod fleet_runtime;
pub mod ideas;
pub mod task_execution;
pub mod worker_profiles;
