//! The coverage rule of the plan validator: each requirement has a task and
//! cites an approved assumption or an answer (FR-3, FM-1, FM-30, I3).

use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

/// The rule name.
pub const RULE: &str = "coverage";

/// Each problem of this rule:
/// - a task covers a requirement that the idea does not have;
/// - a requirement has no task that covers it;
/// - a requirement cites nothing, or cites an item that is not an approved
///   assumption or an answer. A `--direct` idea cites its approved ticket.
pub fn check(plan: &Plan, facts: &PlanFacts) -> Vec<Problem> {
    let mut out = Vec::new();
    let known = |id: &str| facts.requirements.iter().any(|r| r.id == id);
    for task in &plan.tasks {
        for id in task.covers.iter().filter(|id| !known(id)) {
            out.push(Problem::task(
                RULE,
                &task.key,
                format!("the task covers `{id}`, which is not a requirement of the idea"),
            ));
        }
    }
    let approved =
        |cite: &String| facts.approved_assumptions.contains(cite) || facts.answers.contains(cite);
    for req in &facts.requirements {
        let id = &req.id;
        if !plan.tasks.iter().any(|t| t.covers.contains(id)) {
            out.push(Problem::plan(
                RULE,
                format!("requirement {id} has no task that covers it"),
            ));
        }
        if req.cites.is_empty() {
            out.push(Problem::plan(
                RULE,
                format!("requirement {id} cites no approved assumption or answer"),
            ));
        }
        for cite in req.cites.iter().filter(|c| !approved(c)) {
            out.push(Problem::plan(
                RULE,
                format!("requirement {id} cites `{cite}`, which is not an approved assumption or an answer"),
            ));
        }
    }
    out
}
