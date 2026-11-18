//! The model guide rule of the plan validator (D8): Opus 5.5 at medium
//! effort is the default, a higher effort has a stated reason, a Sonnet task
//! is marked `no_decisions: true`, and a Fable task needs `fable: approved`
//! in the plan.
//!
//! A task whose profile is not in the roster is the problem of the profile
//! rule; this rule skips it.

use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

/// The rule name.
pub const RULE: &str = "model_guide";

/// The efforts above the default effort, medium.
const HIGHER_EFFORTS: [&str; 3] = ["high", "xhigh", "max"];

/// Each problem of this rule.
pub fn check(plan: &Plan, facts: &PlanFacts) -> Vec<Problem> {
    let mut out = Vec::new();
    for task in &plan.tasks {
        let Some(profile) = facts.roster.iter().find(|p| p.name == task.profile) else {
            continue;
        };
        let model = profile.model.to_ascii_lowercase();
        let effort = profile.effort.as_deref().unwrap_or("").to_ascii_lowercase();
        let has_reason = task.reason.as_deref().is_some_and(|r| !r.trim().is_empty());
        if HIGHER_EFFORTS.contains(&effort.as_str()) && !has_reason {
            out.push(Problem::task(
                RULE,
                &task.key,
                format!(
                    "profile {} runs at effort {effort}, above the default medium: give a reason",
                    profile.name
                ),
            ));
        }
        if model.contains("sonnet") && !task.no_decisions {
            out.push(Problem::task(
                RULE,
                &task.key,
                format!(
                    "profile {} is a Sonnet profile: mark the task no_decisions: true",
                    profile.name
                ),
            ));
        }
        if model.contains("fable") && !plan.fable_approved {
            out.push(Problem::task(
                RULE,
                &task.key,
                format!(
                    "profile {} is a Fable profile: the plan needs fable: approved",
                    profile.name
                ),
            ));
        }
    }
    out
}
