//! The judge pairing rule of the plan validator: each task of a judgment
//! kind has 1 judge task in another session, on another provider when the
//! roster has one (FR-4, FM-33).
//!
//! A judge task is a task of its own, so it always runs in another session
//! than its author. The rule also runs on a replan, so a change that leaves
//! a judgment task with no judge task is refused (the cancel rule of a
//! judge pair).

use super::plan::{Kind, Plan, PlannedTask};
use super::plan_validator::{PlanFacts, Problem};

/// The rule name.
pub const RULE: &str = "judges";

/// Each problem of this rule.
pub fn check(plan: &Plan, facts: &PlanFacts) -> Vec<Problem> {
    let mut out = Vec::new();
    for task in &plan.tasks {
        match task.kind {
            Some(Kind::Judge) => check_judge(plan, facts, task, &mut out),
            Some(kind) => {
                if task.judges.is_some() {
                    out.push(Problem::task(
                        RULE,
                        &task.key,
                        "only a task of kind judge names judges:",
                    ));
                }
                if kind.is_judgment() {
                    check_judged(plan, task, &mut out);
                }
            }
            None => {}
        }
    }
    out
}

/// A task of a judgment kind has exactly 1 judge task.
fn check_judged(plan: &Plan, task: &PlannedTask, out: &mut Vec<Problem>) {
    let judges = plan
        .tasks
        .iter()
        .filter(|t| t.kind == Some(Kind::Judge) && t.judges.as_deref() == Some(&task.key))
        .count();
    let message = match judges {
        0 => format!(
            "the {} task has no judge task: add a task of kind judge with judges: {}",
            task.kind_word, task.key
        ),
        1 => return,
        n => format!("the task has {n} judge tasks: keep 1"),
    };
    out.push(Problem::task(RULE, &task.key, message));
}

/// A judge task judges 1 task of a judgment kind, on another provider when
fn check_judge(plan: &Plan, facts: &PlanFacts, judge: &PlannedTask, out: &mut Vec<Problem>) {
    let verdict_files = judge.outputs.iter().filter(|o| o.ends_with(".md")).count();
    if judge.outputs.len() != 1 || verdict_files != 1 {
        out.push(Problem::task(
            RULE,
            &judge.key,
            format!(
                "a judge task has exactly 1 output, its .md verdict file; it has {}",
                judge.outputs.len()
            ),
        ));
    }
    let Some(key) = judge.judges.as_deref() else {
        out.push(Problem::task(
            RULE,
            &judge.key,
            "a judge task names the task it judges in judges:",
        ));
        return;
    };
    let Some(judged) = plan.task(key) else {
        out.push(Problem::task(
            RULE,
            &judge.key,
            format!("judges: {key} names no task of the plan"),
        ));
        return;
    };
    if !judged.kind.is_some_and(Kind::is_judgment) {
        out.push(Problem::task(
            RULE,
            &judge.key,
            format!(
                "judges: {key} names a task that is not of a judgment kind ({})",
                judged.kind_word
            ),
        ));
        return;
    }
    let provider = |name: &str| {
        facts
            .roster
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.provider.as_str())
    };
    // An unknown profile is a problem of the profile rule.
    let (Some(author), Some(judge_provider)) =
        (provider(&judged.profile), provider(&judge.profile))
    else {
        return;
    };
    if judge_provider != author {
        return;
    }
    if let Some(other) = facts.roster.iter().find(|p| p.provider != author) {
        out.push(Problem::task(
            RULE,
            &judge.key,
            format!(
                "the judge has the provider {author} of its author, and the roster has the provider {}: use a profile such as {}",
                other.provider, other.name
            ),
        ));
    }
}
