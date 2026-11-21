#![warn(unreachable_pub)]

pub mod clock;
pub mod compaction;
pub mod digest;
pub mod execution;
pub mod exeid;
pub mod fsx;
pub mod harness;
pub mod heartbeat;
pub mod ids;
pub mod messaging;
pub mod procid;
pub mod project_notes;
pub mod prompts;
pub mod roster;
pub mod routing;
pub mod runtime;
pub mod skills;
pub mod sweep;
pub mod taskledger;
pub mod telemetry;
pub mod testkit;
pub mod usage;
pub mod vcs;
pub mod workspace;

pub fn mint_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(all(test, unix))]
pub(crate) mod test_support;

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
