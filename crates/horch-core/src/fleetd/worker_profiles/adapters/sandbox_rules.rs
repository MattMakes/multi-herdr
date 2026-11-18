//! The sandbox rules of profile validation: the call of the operator
//! boundary of each adapter, which refuses a profile whose sandbox allows
//! an operator act or is not supported on this OS.
//!
//! Requirements: SR-8, A37, 06 S9. A worker never runs without its
//! sandbox. A refused profile gets no dispatch and 1 needs-you item
//! (`check_sandbox.rs`), and `horch status` shows it as [`NO_SANDBOX`].
//! An orchestrator seat of `fleet.toml` is not a worker profile, so this
//! rule does not refuse it until Phase 6 (`docs/tickets/fleetd-p5/MAP.md`,
//! Q2).
//!
//! A refusal is a fact of this host (its OS and its CLI versions), not a
//! fault of a teammate file, so it is not a line of `horch teammates
//! --check`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::fleetd::worker_profiles::domain::operator_boundary::{Boundary, NotSupported};
use crate::roster::validation::operator_boundary;
use crate::roster::{Roster, Teammate};

/// What `horch status` shows for a refused profile (design, "The sandbox
/// of a profile denies the operator path").
pub const NO_SANDBOX: &str = "not available on this OS: no sandbox";

/// Why profile validation refuses a worker profile whose adapter gives
/// `boundary`, or `None` when the boundary denies all 3 operator acts.
pub fn refusal_of(boundary: &Result<Boundary, NotSupported>) -> Option<String> {
    match boundary {
        Err(gap) => Some(format!("{NO_SANDBOX}: {gap}")),
        Ok(b) if b.holds() => None,
        Ok(b) => {
            let acts: Vec<String> = b.allowed().iter().map(ToString::to_string).collect();
            Some(format!(
                "{NO_SANDBOX}: its sandbox does not deny the operator acts: {}",
                acts.join(", ")
            ))
        }
    }
}

/// Why profile validation refuses `t` on this OS, or `None`. `seats` are
/// the `orchestrator_seats` of `fleet.toml`; a seat is not refused.
pub fn refusal(t: &Teammate, seats: &[String], fleet_root: &Path) -> Option<String> {
    if seats.contains(&t.name) {
        return None;
    }
    refusal_of(&operator_boundary(t, fleet_root))
}

/// Each refused worker profile of `roster`, by persona, with its reason.
pub fn refused(roster: &Roster, seats: &[String], fleet_root: &Path) -> BTreeMap<String, String> {
    roster
        .names()
        .into_iter()
        .filter_map(|name| {
            let t = roster.get(name)?;
            refusal(t, seats, fleet_root).map(|why| (name.to_string(), why))
        })
        .collect()
}
