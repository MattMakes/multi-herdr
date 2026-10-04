//! Quota-aware routing (A5): which teammate a spawn runs as, and why.
//!
//! Everything here is pure except [`snapshot::obtain`], the only path that
//! probes, and the probes themselves in [`quota_probe`]. Routing never
//! launches a worker (`arc_13_routing_never_launches`).

pub mod balance;
pub mod decision;
pub mod eligible;
pub mod policy;
pub mod quota;
pub mod quota_probe;
pub mod snapshot;
