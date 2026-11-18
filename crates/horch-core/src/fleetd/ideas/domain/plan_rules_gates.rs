//! The gate rule of the plan validator: the project declares a fast gate and
//! a full gate in its `fleet.toml` (FR-3).

use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

/// The rule name.
pub const RULE: &str = "gates";

/// Each problem of this rule: a gate that the project does not declare.
pub fn check(_plan: &Plan, facts: &PlanFacts) -> Vec<Problem> {
    [("fast", &facts.gates.fast), ("full", &facts.gates.full)]
        .into_iter()
        .filter(|(_, cmd)| cmd.as_deref().is_none_or(|c| c.trim().is_empty()))
        .map(|(name, _)| {
            Problem::plan(
                RULE,
                format!("the project declares no {name} gate: add [gates] {name} to fleet.toml"),
            )
        })
        .collect()
}
