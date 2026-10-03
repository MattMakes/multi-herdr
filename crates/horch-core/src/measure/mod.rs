//! Measurement foundation for the dataset (phase B1): digests, the in-repo
//! PRNG, secret redaction, the dataset paths, the event log and its
//! projections.

pub mod digest;
pub mod event;
pub mod paths;
pub mod projection;
pub mod recorder;
pub mod redact;
pub mod store;
pub mod testkit;
