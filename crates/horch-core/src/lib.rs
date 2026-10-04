//! Shared machinery for the `horch` multi-agent orchestration CLI.
//!
//! Each module owns one part of the domain. Callers import an item from the
//! module that owns it: there are no re-export shims (ARC-25), and an item
//! nothing outside this crate uses is `pub(crate)`.
//!
//! | module          | owns                                                        |
//! |-----------------|-------------------------------------------------------------|
//! | [`ids`]         | typed identities (`ExecutionId`, `RoleName`, `PaneId`, ...) |
//! | [`clock`]       | the one clock horch reads (`HORCH_NOW` pins it)             |
//! | [`fsx`]         | durable file writes and the cross-process directory lock    |
//! | [`runtime`]     | the process boundary: `RuntimeContext`, paths, binaries     |
//! | [`roster`]      | `teammates/` files: parsing, layering, `--check` rules      |
//! | [`prompts`]     | rendering briefings from `teammates/`                       |
//! | [`skills`]      | skill catalogs, activation plans and launch bundles         |
//! | [`harness`]     | the agent CLIs (claude, codex, OpenCode, pi, Prime, Antigravity) and launch |
//! | [`routing`]     | quota-aware routing: policy, quota view, decisions          |
//! | [`execution`]   | executions: model, ledger record and store, spawn plan, lifecycle |
//! | [`messaging`]   | the worker brief, the mailbox and message delivery          |
//! | [`workspace`]   | the herdr client, layout, tiling and balancing              |
//! | [`telemetry`]   | live per-pane token telemetry and the collector             |
//! | [`usage`]       | what a run cost, read back from each harness's own records  |
//! | [`vcs`]         | typed git and worktrees for the dataset mode                |
//! | [`measure`]     | dataset digests, events, the event store and projections    |
//! | [`competition`] | competitive mode: config, preflight, planner, coordinator, promotion |
//! | [`evaluation`]  | validation gates, judge input, judging and winner policy    |
//! | [`dataset`]     | the dataset outputs: export rows and readiness              |
//! | [`teacher`]     | the System One decision-model seam (inert)                  |
//!
//! Nothing here shells out to `bash`, `jq`, `node`, or `just`. The external
//! processes are `herdr`, the agent CLI a worker launches, `git` in the
//! dataset mode, and `sqlite3` for OpenCode's usage database.

#![warn(unreachable_pub)]

pub mod clock;
pub mod competition;
pub mod dataset;
pub mod evaluation;
pub mod execution;
pub mod fsx;
pub mod harness;
pub mod ids;
pub mod measure;
pub mod messaging;
pub mod prompts;
pub mod roster;
pub mod routing;
pub mod runtime;
pub mod skills;
pub mod teacher;
pub mod telemetry;
pub mod usage;
pub mod vcs;
pub mod workspace;

/// Mint a session or record id.
pub fn mint_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_uuids_are_lowercase_and_unique() {
        let a = mint_uuid();
        let b = mint_uuid();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(a, a.to_lowercase());
    }
}
