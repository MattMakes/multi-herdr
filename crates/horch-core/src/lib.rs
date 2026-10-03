//! Shared machinery for the `horch` multi-agent orchestration CLI.
//!
//! This is the Rust port of the bash/jq implementation. Every module maps to
//! something that used to be a script:
//!
//! | module      | replaced                                        |
//! |-------------|-------------------------------------------------|
//! | [`herdr`]   | `herdr ... \| jq` pipelines                      |
//! | [`mailbox`] | `scripts/lib/herdr-register.sh`, `*.brief` files |
//! | [`ledger`]  | `bin/horch-ledger`                              |
//! | [`layout`]  | `bin/horch-layout`                              |
//! | [`prompts`] | the launcher heredocs (now rendered from `teammates/`) |
//! | [`codex`]   | `worker.sh`'s execpolicy and harvest logic       |
//!
//! Nothing here shells out to `bash`, `jq`, `node`, or `just`: the only external
//! process is `herdr` itself, plus whichever agent CLI a worker launches.

pub mod clock;
pub mod competition;
pub mod dataset;
pub mod evaluation;
pub mod execution;
pub mod fsx;
pub mod harness;
pub mod ids;
pub mod ledger;
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
