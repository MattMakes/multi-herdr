use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem, RosterProfile};

pub const RULE: &str = "profiles";

enum Finding {
    Refusal(String),
    Note(String),
}

pub fn check(plan: &Plan, facts: &PlanFacts) -> Vec<Problem> {
    findings(plan, facts, |f| match f {
        Finding::Refusal(m) => Some(m),
        Finding::Note(_) => None,
    })
}

pub fn notes(plan: &Plan, facts: &PlanFacts) -> Vec<Problem> {
    findings(plan, facts, |f| match f {
        Finding::Note(m) => Some(m),
        Finding::Refusal(_) => None,
    })
}

fn findings(plan: &Plan, facts: &PlanFacts, keep: fn(Finding) -> Option<String>) -> Vec<Problem> {
    let mut out = Vec::new();
    for task in &plan.tasks {
        if task.profile.trim().is_empty() || task.key.trim().is_empty() {
            continue;
        }
        for finding in of_profile(&task.profile, &facts.roster) {
            if let Some(message) = keep(finding) {
                out.push(Problem::task(RULE, &task.key, message));
            }
        }
    }
    out
}

fn find<'a>(roster: &'a [RosterProfile], name: &str) -> Option<&'a RosterProfile> {
    roster.iter().find(|p| p.name == name)
}

fn of_profile(name: &str, roster: &[RosterProfile]) -> Vec<Finding> {
    let Some(profile) = find(roster, name) else {
        return vec![Finding::Refusal(format!(
            "`{name}` is not a profile of the roster"
        ))];
    };
    let mut out = Vec::new();
    let fallback = match profile.fallback.as_deref().map(str::trim) {
        None | Some("") => {
            out.push(Finding::Refusal(format!(
                "profile `{name}` names no fallback"
            )));
            None
        }
        Some(fb) => match find(roster, fb) {
            Some(p) => Some(p),
            None => {
                out.push(Finding::Refusal(format!(
                    "the fallback `{fb}` of profile `{name}` is not a profile of the roster"
                )));
                None
            }
        },
    };
    if !profile.sandbox_here {
        let no_sandbox = format!("profile `{name}` is not available on this OS: no sandbox");
        out.push(match fallback {
            Some(fb) if fb.sandbox_here => Finding::Note(format!(
                "{no_sandbox}; its fallback `{}` runs the task",
                fb.name
            )),
            _ => Finding::Refusal(format!(
                "{no_sandbox}, and it has no fallback with a sandbox"
            )),
        });
    }
    out
}
