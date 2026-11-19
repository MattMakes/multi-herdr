//! The plan validator frame: it runs each plan rule and collects the
//! verdict (FR-3, FR-4).
//!
//! `PlanValidator` is pure: no file, no ledger. The reader of plan.yaml
//! gives the `Plan`, and the caller gives the `PlanFacts` that the rules
//! need. Each rule is 1 function of its own file; this file holds the list
//! of rules and the schema rule.

use super::plan::{Acceptance, Kind, Plan};
use super::{
    plan_rules_coverage, plan_rules_dag, plan_rules_gates, plan_rules_judges,
    plan_rules_model_guide, plan_rules_profiles,
};

/// 1 problem of a plan, as plan.verdict.json lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The rule that found it: `schema`, `dag`, `coverage`, ...
    pub rule: &'static str,
    /// The key of the task, or `None` for the plan as a whole.
    pub task: Option<String>,
    pub message: String,
}

impl Problem {
    /// A problem of the task of `key`.
    pub fn task(rule: &'static str, key: &str, message: impl Into<String>) -> Problem {
        Problem {
            rule,
            task: Some(key.to_string()),
            message: message.into(),
        }
    }

    /// A problem of the plan as a whole.
    pub fn plan(rule: &'static str, message: impl Into<String>) -> Problem {
        Problem {
            rule,
            task: None,
            message: message.into(),
        }
    }
}

/// 1 requirement of requirements.yaml.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Requirement {
    /// `R<n>`.
    pub id: String,
    pub text: String,
    /// The items that it cites: an assumption `A<n>` or an answer `Q<n>`.
    pub cites: Vec<String>,
}

/// 1 profile of the roster, as the profile rules need it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RosterProfile {
    pub name: String,
    /// The agent of the profile: `claude`, `codex`, ...
    pub provider: String,
    /// The model of the profile, as the roster names it.
    pub model: String,
    /// The effort of the profile, when the roster sets one.
    pub effort: Option<String>,
    /// The fallback profile.
    pub fallback: Option<String>,
    /// Profile validation finds a sandbox for it on this OS.
    pub sandbox_here: bool,
}

/// The gates that the project declares in its `fleet.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeclaredGates {
    pub fast: Option<String>,
    pub full: Option<String>,
}

/// What the plan rules need besides the plan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanFacts {
    /// The requirements of the idea (requirements.yaml).
    pub requirements: Vec<Requirement>,
    /// The ids of the assumptions that the operator approved (`A<n>`).
    pub approved_assumptions: Vec<String>,
    /// The ids of the answers of the operator (`Q<n>` of interview.md).
    pub answers: Vec<String>,
    /// The profiles of the roster.
    pub roster: Vec<RosterProfile>,
    /// The gates that the project declares.
    pub gates: DeclaredGates,
}

/// A plan rule: each problem that it finds in the plan.
pub type Rule = fn(&Plan, &PlanFacts) -> Vec<Problem>;

/// The rule name of the schema rule.
pub const SCHEMA: &str = "schema";

/// Each rule, in the order of the verdict.
pub const RULES: &[Rule] = &[
    schema,
    plan_rules_dag::check,
    plan_rules_coverage::check,
    plan_rules_profiles::check,
    plan_rules_gates::check,
    plan_rules_model_guide::check,
    plan_rules_judges::check,
];

/// The plan validator.
pub struct PlanValidator;

impl PlanValidator {
    /// Each problem of `plan`: an empty list is a valid plan.
    pub fn validate(plan: &Plan, facts: &PlanFacts) -> Vec<Problem> {
        RULES.iter().flat_map(|rule| rule(plan, facts)).collect()
    }
}

/// The schema rule: the plan names its idea, repo, feature and branch, and
/// has a task; each task has a unique key, a title, a known kind, a
/// profile, an acceptance check (FR-4) and a ticket in format C1 when it
/// names one. A judge task has no acceptance check of its own: its verdict
/// is its check.
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

/// Why the output path `path` of plan.yaml, an untrusted input, is not a
/// relative path inside the repository. `None`: it is.
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
