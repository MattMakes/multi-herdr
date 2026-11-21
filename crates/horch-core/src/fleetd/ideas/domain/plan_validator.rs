use super::plan::{Acceptance, Kind, Plan};
use super::{
    plan_rules_coverage, plan_rules_dag, plan_rules_gates, plan_rules_judges,
    plan_rules_model_guide, plan_rules_profiles,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub rule: &'static str,
    pub task: Option<String>,
    pub message: String,
}

impl Problem {
    pub fn task(rule: &'static str, key: &str, message: impl Into<String>) -> Problem {
        Problem {
            rule,
            task: Some(key.to_string()),
            message: message.into(),
        }
    }

    pub fn plan(rule: &'static str, message: impl Into<String>) -> Problem {
        Problem {
            rule,
            task: None,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Requirement {
    pub id: String,
    pub text: String,
    pub cites: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RosterProfile {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub effort: Option<String>,
    pub fallback: Option<String>,
    pub sandbox_here: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeclaredGates {
    pub fast: Option<String>,
    pub full: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanFacts {
    pub requirements: Vec<Requirement>,
    pub approved_assumptions: Vec<String>,
    pub answers: Vec<String>,
    pub roster: Vec<RosterProfile>,
    pub gates: DeclaredGates,
}

pub type Rule = fn(&Plan, &PlanFacts) -> Vec<Problem>;

pub const SCHEMA: &str = "schema";

pub const RULES: &[Rule] = &[
    schema,
    plan_rules_dag::check,
    plan_rules_coverage::check,
    plan_rules_profiles::check,
    plan_rules_gates::check,
    plan_rules_model_guide::check,
    plan_rules_judges::check,
];

pub struct PlanValidator;

impl PlanValidator {
    pub fn validate(plan: &Plan, facts: &PlanFacts) -> Vec<Problem> {
        RULES.iter().flat_map(|rule| rule(plan, facts)).collect()
    }
}

pub fn schema(plan: &Plan, _facts: &PlanFacts) -> Vec<Problem> {
    let mut out = Vec::new();
    for (field, value) in [
        ("idea", &plan.idea),
        ("repo", &plan.repo),
        ("feature", &plan.feature),
        ("base_branch", &plan.base_branch),
    ] {
        if value.trim().is_empty() {
            out.push(Problem::plan(SCHEMA, format!("the plan has no {field}")));
        }
    }
    if plan.tasks.is_empty() {
        out.push(Problem::plan(SCHEMA, "the plan has no task"));
    }
    for (i, task) in plan.tasks.iter().enumerate() {
        let key = task.key.as_str();
        if key.trim().is_empty() {
            out.push(Problem::plan(
                SCHEMA,
                format!("task {} of the plan has no key", i + 1),
            ));
            continue;
        }
        let mut add = |message: String| out.push(Problem::task(SCHEMA, key, message));
        let same = plan.tasks.iter().filter(|t| t.key == key).count();
        if same > 1 && plan.tasks.iter().position(|t| t.key == key) == Some(i) {
            add(format!("{same} tasks have the key `{key}`"));
        }
        if task.title.trim().is_empty() {
            add("the task has no title".into());
        }
        if task.kind.is_none() {
            let words: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
            add(format!(
                "`{}` is not a kind; use one of: {}",
                task.kind_word,
                words.join(", ")
            ));
        }
        if task.profile.trim().is_empty() {
            add("the task names no profile".into());
        }
        let judge = task.kind == Some(Kind::Judge);
        if task.acceptance.is_empty() && !judge {
            add("the task has no acceptance check: add a run: or a sections: item".into());
        }
        for item in &task.acceptance {
            match item {
                Acceptance::Run(cmd) if cmd.trim().is_empty() => {
                    add("an acceptance item has an empty run: command".into())
                }
                Acceptance::Sections { file, .. } if file.trim().is_empty() => {
                    add("an acceptance sections: item names no file".into())
                }
                _ => {}
            }
        }
        if let Some(ticket) = &task.ticket {
            for p in &ticket.problems {
                add(format!("ticket {}: {p}", ticket.path));
            }
        }
        for path in &task.outputs {
            if let Some(why) = unsafe_output(path) {
                add(format!("the output `{path}` {why}"));
            }
            let first = plan
                .tasks
                .iter()
                .position(|t| t.outputs.contains(path))
                .is_some_and(|f| f == i);
            let owners = plan
                .tasks
                .iter()
                .filter(|t| t.outputs.contains(path))
                .count();
            if first && owners > 1 {
                add(format!(
                    "{owners} tasks have the output `{path}`: each output path is unique in the plan"
                ));
            }
        }
    }
    out
}

fn unsafe_output(path: &str) -> Option<&'static str> {
    if path.trim().is_empty() {
        return Some("is empty");
    }
    if path.starts_with('/') || path.starts_with('\\') || path.contains(':') {
        return Some("is not a relative path");
    }
    if path.split(['/', '\\']).any(|part| part == "..") {
        return Some("goes out of the repository with `..`");
    }
    None
}
