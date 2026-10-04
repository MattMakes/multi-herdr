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
pub mod worker_run;

use serde::{Deserialize, Serialize};

/// One line of `git diff --numstat`. A binary file has no line counts. It is
/// a domain value: the events, the projection and WorkerRun carry it, and
/// the git adapter (`vcs::git`) produces it (CMP-02).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumstatLine {
    pub added: Option<u32>,
    pub deleted: Option<u32>,
    pub path: String,
}
