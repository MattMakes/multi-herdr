//! The reader of plan.yaml.
//!
//! It reads plan.yaml into a `Plan`, and each ticket that a task names,
//! with the C1 checks of `taskledger::map::ticket_problems`. A file that is
//! not YAML of the plan shape (an unknown field, a value of the wrong type)
//! is an error. A value that YAML allows and the plan rules refuse (an
//! unknown kind, an empty field, a ticket that is not C1) stays in the
//! `Plan`, for the plan validator to report.

use std::path::{Component, Path};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::fleetd::ideas::domain::plan::{Acceptance, Budget, Kind, Plan, PlannedTask, TicketFile};
use crate::taskledger::map::ticket_problems;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanKeys {
    #[serde(default)]
    idea: String,
    #[serde(default)]
    repo: String,
    #[serde(default)]
    feature: String,
    #[serde(default)]
    base_branch: String,
    fable: Option<String>,
    #[serde(default)]
    tasks: Vec<TaskKeys>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskKeys {
    #[serde(default)]
    key: String,
    #[serde(default)]
    title: String,
    ticket: Option<String>,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    profile: String,
    #[serde(default)]
    covers: Vec<String>,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    outputs: Vec<String>,
    #[serde(default)]
    acceptance: Vec<AcceptanceKeys>,
    #[serde(default)]
    after: Vec<String>,
    judges: Option<String>,
    budget: Option<BudgetKeys>,
    reason: Option<String>,
    #[serde(default)]
    no_decisions: bool,
}

/// 1 acceptance item: exactly 1 of its keys.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptanceKeys {
    run: Option<String>,
    /// The file first, then each section.
    sections: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetKeys {
    max_minutes: Option<u32>,
}

/// Read the plan.yaml at `path` and each ticket that it names. A ticket path
/// is relative to the folder of plan.yaml.
pub fn read(path: &Path) -> Result<Plan> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut plan = parse(&text).with_context(|| format!("{}", path.display()))?;
    let folder = path.parent().unwrap_or(Path::new("."));
    for task in &mut plan.tasks {
        if let Some(ticket) = &mut task.ticket {
            ticket.problems = ticket_file_problems(folder, &ticket.path);
        }
    }
    Ok(plan)
}

/// Parse the text of a plan.yaml. Each ticket is named, not read: its
/// `problems` are empty.
pub fn parse(text: &str) -> Result<Plan> {
    let keys: PlanKeys = serde_yaml::from_str(text).context("plan.yaml")?;
    let fable_approved = match keys.fable.as_deref() {
        None => false,
        Some("approved") => true,
        Some(word) => anyhow::bail!("plan.yaml: fable: `{word}` is not approved"),
    };
    Ok(Plan {
        idea: keys.idea,
        repo: keys.repo,
        feature: keys.feature,
        base_branch: keys.base_branch,
        fable_approved,
        tasks: keys
            .tasks
            .into_iter()
            .enumerate()
            .map(|(i, t)| task(t).with_context(|| format!("plan.yaml: tasks[{i}]")))
            .collect::<Result<_>>()?,
    })
}

fn task(keys: TaskKeys) -> Result<PlannedTask> {
    let acceptance = keys
        .acceptance
        .into_iter()
        .enumerate()
        .map(|(i, a)| match (a.run, a.sections) {
            (Some(cmd), None) => Ok(Acceptance::Run(cmd)),
            (None, Some(mut items)) => {
                let file = if items.is_empty() {
                    String::new()
                } else {
                    items.remove(0)
                };
                Ok(Acceptance::Sections {
                    file,
                    sections: items,
                })
            }
            _ => anyhow::bail!("acceptance[{i}]: an item has 1 key, run or sections"),
        })
        .collect::<Result<_>>()?;
    Ok(PlannedTask {
        kind: Kind::parse(&keys.kind),
        kind_word: keys.kind,
        key: keys.key,
        title: keys.title,
        ticket: keys.ticket.map(|path| TicketFile {
            path,
            problems: Vec::new(),
        }),
        profile: keys.profile,
        covers: keys.covers,
        inputs: keys.inputs,
        outputs: keys.outputs,
        acceptance,
        after: keys.after,
        judges: keys.judges,
        budget: keys.budget.map(|b| Budget {
            max_minutes: b.max_minutes,
        }),
        reason: keys.reason,
        no_decisions: keys.no_decisions,
    })
}

/// What is wrong with the ticket `rel` of the plan folder `folder`: it is
/// not a relative path inside the folder, it cannot be read, or each C1
/// problem with its line.
fn ticket_file_problems(folder: &Path, rel: &str) -> Vec<String> {
    let inside = !rel.is_empty()
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !inside {
        return vec!["the path is not a relative path inside the plan folder".into()];
    }
    match std::fs::read_to_string(folder.join(rel)) {
        Err(e) => vec![format!("cannot be read: {e}")],
        Ok(text) => ticket_problems(&text)
            .into_iter()
            .map(|(line, p)| format!("line {line}: {p}"))
            .collect(),
    }
}
