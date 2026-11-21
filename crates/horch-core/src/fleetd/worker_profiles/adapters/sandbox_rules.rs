use std::collections::BTreeMap;
use std::path::Path;

use crate::fleetd::worker_profiles::domain::operator_boundary::{Boundary, NotSupported};
use crate::roster::validation::operator_boundary;
use crate::roster::{Roster, Teammate};

pub const NO_SANDBOX: &str = "not available on this OS: no sandbox";

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

pub fn refusal(t: &Teammate, fleet_root: &Path) -> Option<String> {
    refusal_of(&operator_boundary(t, fleet_root))
}

pub fn refused(roster: &Roster, fleet_root: &Path) -> BTreeMap<String, String> {
    roster
        .names()
        .into_iter()
        .filter_map(|name| {
            let t = roster.get(name)?;
            refusal(t, fleet_root).map(|why| (name.to_string(), why))
        })
        .collect()
}
