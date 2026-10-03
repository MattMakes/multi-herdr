//! Moved to [`crate::routing`] (A5): the pure parts to `routing::quota`, the
//! probes to `routing::quota_probe`, and `current_view` to
//! `routing::snapshot::obtain`. This shim keeps old paths compiling until A12.

pub use crate::routing::quota::*;
pub use crate::routing::quota_probe::*;
pub use crate::routing::snapshot::obtain as current_view;
pub use crate::routing::snapshot::QuotaEnv;
