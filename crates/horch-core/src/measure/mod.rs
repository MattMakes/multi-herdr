//! Measurement foundation for the dataset (phase B1): digests, the in-repo
//! PRNG, secret redaction, the dataset paths, the event log and its
//! projections.

pub mod digest;
pub mod envsnap;
pub mod event;
pub mod paths;
pub mod projection;
pub mod recorder;
pub mod redact;
pub mod store;
pub mod testkit;

/// `git diff --numstat` lines, re-exported so the domain modules (events,
/// projection, WorkerRun) name the value type without naming the git
/// adapter module (CMP-02). The type belongs in a domain module; B3 may move it.
pub use crate::vcs::git::NumstatLine;
