//! The judge pairing rule of the plan validator (ticket 12 of fleetd-p6;
//! FR-4, FM-33, the cancel rule of a judge pair).

use horch_core::fleetd::ideas::domain::plan::{Acceptance, Kind, Plan, PlannedTask};
use horch_core::fleetd::ideas::domain::plan_rules_judges::{check, RULE};
use horch_core::fleetd::ideas::domain::plan_validator::{
    PlanFacts, PlanValidator, Problem, RosterProfile,
};

fn profile(name: &str, provider: &str) -> RosterProfile {
    RosterProfile {
        name: name.into(),
        provider: provider.into(),
        model: "m".into(),
        sandbox_here: true,
        ..RosterProfile::default()
    }
}

/// A roster with 2 claude profiles and 1 codex profile.
fn facts() -> PlanFacts {
    PlanFacts {
        roster: vec![
            profile("writer-claude", "claude"),
            profile("judge-claude", "claude"),
            profile("judge-codex", "codex"),
        ],
        ..PlanFacts::default()
    }
}

fn task(key: &str, kind: Kind, profile: &str) -> PlannedTask {
    PlannedTask {
        key: key.into(),
        title: format!("Task {key}"),
        kind: Some(kind),
        kind_word: kind.as_str().into(),
        profile: profile.into(),
        acceptance: vec![Acceptance::Run("true".into())],
        ..PlannedTask::default()
    }
}

/// A judge task with its 1 output, its verdict file.
fn judge(key: &str, judges: &str, profile: &str) -> PlannedTask {
    PlannedTask {
        judges: Some(judges.into()),
        acceptance: Vec::new(),
        outputs: vec![format!("{judges}.review.md")],
        ..task(key, Kind::Judge, profile)
    }
}

fn plan(tasks: Vec<PlannedTask>) -> Plan {
    Plan {
        idea: "I-0001".into(),
        repo: "/fleet/repos/app".into(),
        feature: "F-0001".into(),
        base_branch: "feature/x".into(),
        tasks,
        ..Plan::default()
    }
}

fn judge_problems(problems: &[Problem]) -> Vec<&Problem> {
    problems.iter().filter(|p| p.rule == RULE).collect()
}

#[test]
fn plan_without_paired_judge_is_refused() {
    let p = plan(vec![
        task("doc", Kind::Docs, "writer-claude"),
        task("code", Kind::Feature, "writer-claude"),
    ]);
    let problems = check(&p, &facts());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].task.as_deref(), Some("doc"));
    assert!(
        problems[0].message.contains("no judge task"),
        "{problems:?}"
    );

    // The validator runs the rule.
    let all = PlanValidator::validate(&p, &facts());
    assert_eq!(judge_problems(&all).len(), 1, "{all:?}");

    // Each judgment kind needs a judge.
    for kind in Kind::ALL.into_iter().filter(|k| k.is_judgment()) {
        let p = plan(vec![task("j", kind, "writer-claude")]);
        assert_eq!(check(&p, &facts()).len(), 1, "{kind:?}");
    }

    // With its judge the plan passes the rule.
    let p = plan(vec![
        task("doc", Kind::Docs, "writer-claude"),
        judge("doc-judge", "doc", "judge-codex"),
    ]);
    assert!(check(&p, &facts()).is_empty());
    let all = PlanValidator::validate(&p, &facts());
    assert!(judge_problems(&all).is_empty(), "{all:?}");
}

#[test]
fn judge_of_a_code_task_is_refused() {
    let p = plan(vec![
        task("code", Kind::Feature, "writer-claude"),
        judge("code-judge", "code", "judge-codex"),
    ]);
    let problems = check(&p, &facts());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].task.as_deref(), Some("code-judge"));
    assert!(problems[0].message.contains("not of a judgment kind"));

    // A judge of an unknown key, and a judge that names no task.
    let mut lost = judge("lost-judge", "nowhere", "judge-codex");
    let p = plan(vec![lost.clone()]);
    assert_eq!(check(&p, &facts()).len(), 1);
    lost.judges = None;
    let p = plan(vec![lost]);
    let problems = check(&p, &facts());
    assert_eq!(problems.len(), 1);
    assert!(problems[0].message.contains("judges:"));

    // Only a judge task names `judges:`.
    let mut code = task("code", Kind::Feature, "writer-claude");
    code.judges = Some("doc".into());
    let p = plan(vec![
        task("doc", Kind::Docs, "writer-claude"),
        judge("doc-judge", "doc", "judge-codex"),
        code,
    ]);
    let problems = check(&p, &facts());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].task.as_deref(), Some("code"));

    // 2 judges of 1 task.
    let p = plan(vec![
        task("doc", Kind::Docs, "writer-claude"),
        judge("doc-judge", "doc", "judge-codex"),
        judge("doc-judge-2", "doc", "judge-codex"),
    ]);
    let problems = check(&p, &facts());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].task.as_deref(), Some("doc"));
}

#[test]
fn judge_on_the_same_provider_is_refused_when_another_exists() {
    let p = plan(vec![
        task("doc", Kind::Docs, "writer-claude"),
        judge("doc-judge", "doc", "judge-claude"),
    ]);
    let problems = check(&p, &facts());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].task.as_deref(), Some("doc-judge"));
    assert!(problems[0].message.contains("codex"), "{problems:?}");

    // A roster of 1 provider: the same provider is allowed.
    let one = PlanFacts {
        roster: vec![
            profile("writer-claude", "claude"),
            profile("judge-claude", "claude"),
        ],
        ..PlanFacts::default()
    };
    assert!(check(&p, &one).is_empty());
}

#[test]
fn judge_needs_no_judge() {
    let p = plan(vec![
        task("design", Kind::Design, "writer-claude"),
        judge("design-judge", "design", "judge-codex"),
    ]);
    assert!(check(&p, &facts()).is_empty());
}

#[test]
fn replan_that_leaves_a_judgment_task_without_judge_is_refused() {
    let before = plan(vec![
        task("doc", Kind::Report, "writer-claude"),
        judge("doc-judge", "doc", "judge-codex"),
        task("code", Kind::Feature, "writer-claude"),
    ]);
    assert!(check(&before, &facts()).is_empty());

    // The replan replaces the judge task and keeps the judged task.
    let mut after = before.clone();
    after.tasks.retain(|t| t.key != "doc-judge");
    after
        .tasks
        .push(task("code-2", Kind::Feature, "writer-claude"));
    let problems = check(&after, &facts());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].task.as_deref(), Some("doc"));
}

/// A judge task writes exactly 1 verdict file: its 1 `.md` output (fault 8
/// of T-561; ticket 39 of fleetd-p6).
#[test]
fn judge_without_exactly_1_md_output_is_refused() {
    for outputs in [
        vec![],
        vec!["doc.review.txt".to_string()],
        vec!["a.md".into(), "b.md".into()],
    ] {
        let mut j = judge("doc-judge", "doc", "judge-codex");
        j.outputs = outputs.clone();
        let p = plan(vec![task("doc", Kind::Docs, "writer-claude"), j]);
        let problems = check(&p, &facts());
        assert_eq!(problems.len(), 1, "{outputs:?}: {problems:?}");
        assert_eq!(problems[0].task.as_deref(), Some("doc-judge"));
        assert!(
            problems[0].message.contains("exactly 1 output"),
            "{problems:?}"
        );
    }
}
