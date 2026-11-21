use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

pub const RULE: &str = "gates";

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
