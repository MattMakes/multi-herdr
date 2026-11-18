//! The judge pairing rule of the plan validator: each task of a judgment
//! kind has 1 judge task in another session.
//!
//! Ticket 12 of fleetd-p6 fills it.

use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

/// The rule name.
pub const RULE: &str = "judges";

/// Each problem of this rule. The rule is not written yet: it finds none.
pub fn check(_plan: &Plan, _facts: &PlanFacts) -> Vec<Problem> {
    Vec::new()
}
