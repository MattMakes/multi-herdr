//! The coverage rule of the plan validator: each requirement has a task and
//! cites an approved assumption or an answer.
//!
//! Ticket 09 of fleetd-p6 fills it.

use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

/// The rule name.
pub const RULE: &str = "coverage";

/// Each problem of this rule. The rule is not written yet: it finds none.
pub fn check(_plan: &Plan, _facts: &PlanFacts) -> Vec<Problem> {
    Vec::new()
}
