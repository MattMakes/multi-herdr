//! The facts of a live attempt that the authorization of a request reads.
//!
//! No attempt has operator rights: Phase 6 removed the orchestrator seat
//! (SR-8, FM-32; ticket 44 of fleetd-p6).

/// The facts of 1 live attempt, as the ledger records them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptFacts {
    /// The id of the attempt.
    pub attempt: String,
    /// The task of the attempt. `None` when the ledger row has a null task.
    pub task: Option<String>,
    /// The persona of the attempt, as the ledger records it.
    pub persona: String,
}
