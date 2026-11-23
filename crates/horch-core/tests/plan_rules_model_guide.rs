use horch_core::fleetd::ideas::domain::plan::{Acceptance, Kind, Plan, PlannedTask};
use horch_core::fleetd::ideas::domain::plan_rules_model_guide::RULE;
use horch_core::fleetd::ideas::domain::plan_validator::{
    PlanFacts, PlanValidator, Problem, RosterProfile,
};

fn profile(name: &str, model: &str, effort: &str) -> RosterProfile {
    RosterProfile {
        name: name.into(),
        provider: "claude".into(),
        model: model.into(),
        effort: Some(effort.into()),
        fallback: None,
        sandbox_here: true,
    }
}

fn facts() -> PlanFacts {
    PlanFacts {
        roster: vec![
            profile("opus", "claude-opus-5-5", "medium"),
            profile("opus-high", "claude-opus-5-5", "high"),
            profile("sonnet", "claude-sonnet-5-5", "medium"),
            profile("fable", "claude-fable-5-2", "medium"),
        ],
        ..PlanFacts::default()
    }
}

fn plan(profile: &str) -> Plan {
    Plan {
        idea: "I-0001".into(),
        repo: "/fleet/repos/app".into(),
        feature: "F-0001".into(),
        base_branch: "feature/a".into(),
        fable_approved: false,
        tasks: vec![PlannedTask {
            key: "a".into(),
            title: "Task a".into(),
            kind: Some(Kind::Feature),
            kind_word: "feature".into(),
            profile: profile.into(),
            acceptance: vec![Acceptance::Run("true".into())],
            ..PlannedTask::default()
        }],
    }
}

fn problems(plan: &Plan) -> Vec<Problem> {
    PlanValidator::validate(plan, &facts())
        .into_iter()
        .filter(|p| p.rule == RULE)
        .collect()
}

fn assert_one(plan: &Plan, words: &str) {
    let found = problems(plan);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].task.as_deref(), Some("a"));
    assert!(found[0].message.contains(words), "{found:?}");
}

#[test]
fn default_opus_medium_task_is_valid() {
    assert_eq!(problems(&plan("opus")), vec![]);
}

#[test]
fn high_effort_without_reason_is_refused() {
    let mut p = plan("opus-high");
    assert_one(&p, "reason");
    p.tasks[0].reason = Some("  ".into());
    assert_one(&p, "reason");
    p.tasks[0].reason = Some("The parser change needs deep thought.".into());
    assert_eq!(problems(&p), vec![]);
}

#[test]
fn sonnet_task_without_no_decisions_is_refused() {
    let mut p = plan("sonnet");
    assert_one(&p, "no_decisions");
    p.tasks[0].no_decisions = true;
    assert_eq!(problems(&p), vec![]);
}

#[test]
fn fable_task_without_your_flag_is_refused() {
    let mut p = plan("fable");
    assert_one(&p, "fable: approved");
    p.fable_approved = true;
    assert_eq!(problems(&p), vec![]);
}
