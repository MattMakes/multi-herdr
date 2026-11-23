use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

pub const RULE: &str = "coverage";

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
