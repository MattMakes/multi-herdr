//! The model guide rule of the plan validator: the default model and
//! effort, and the reason for each other choice.
//!
//! Ticket 11 of fleetd-p6 fills it.

use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

/// The rule name.
pub const RULE: &str = "model_guide";

/// Each problem of this rule. The rule is not written yet: it finds none.
pub fn check(_plan: &Plan, _facts: &PlanFacts) -> Vec<Problem> {
    Vec::new()
}
