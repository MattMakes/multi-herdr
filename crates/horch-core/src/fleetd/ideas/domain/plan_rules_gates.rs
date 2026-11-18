//! The gate rule of the plan validator: the project declares a fast gate and
//! a full gate.
//!
//! Ticket 10 of fleetd-p6 fills it.

use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

/// The rule name.
pub const RULE: &str = "gates";

/// Each problem of this rule. The rule is not written yet: it finds none.
pub fn check(_plan: &Plan, _facts: &PlanFacts) -> Vec<Problem> {
    Vec::new()
}
