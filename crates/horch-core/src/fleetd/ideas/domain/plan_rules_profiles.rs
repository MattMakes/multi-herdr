//! The profile rule of the plan validator: each profile exists, has a
//! fallback and has a sandbox on this OS.
//!
//! Ticket 10 of fleetd-p6 fills it.

use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

/// The rule name.
pub const RULE: &str = "profiles";

/// Each problem of this rule. The rule is not written yet: it finds none.
pub fn check(_plan: &Plan, _facts: &PlanFacts) -> Vec<Problem> {
    Vec::new()
}
