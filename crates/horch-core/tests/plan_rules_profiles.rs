use horch_core::fleetd::ideas::domain::plan::{Acceptance, Kind, Plan, PlannedTask};
use horch_core::fleetd::ideas::domain::plan_rules_profiles;
use horch_core::fleetd::ideas::domain::plan_validator::{
    DeclaredGates, PlanFacts, PlanValidator, Problem, RosterProfile,
};

fn task(key: &str, profile: &str) -> PlannedTask {
    PlannedTask {
        key: key.into(),
        title: format!("Task {key}"),
        kind: Some(Kind::Feature),
        kind_word: "feature".into(),
        profile: profile.into(),
        covers: vec!["R1".into()],
        acceptance: vec![Acceptance::Run("cargo test".into())],
        ..PlannedTask::default()
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

fn profile(name: &str, fallback: Option<&str>, sandbox_here: bool) -> RosterProfile {
    RosterProfile {
        name: name.into(),
        provider: "claude".into(),
        model: "opus".into(),
        fallback: fallback.map(String::from),
        sandbox_here,
        ..RosterProfile::default()
    }
}

fn facts(roster: Vec<RosterProfile>) -> PlanFacts {
    PlanFacts {
        roster,
        gates: DeclaredGates {
            fast: Some("cargo test --lib".into()),
            full: Some("cargo test".into()),
        },
        ..PlanFacts::default()
    }
}

fn good_roster() -> Vec<RosterProfile> {
    vec![
        profile("opus", Some("codex-sol"), true),
        profile("codex-sol", Some("opus"), true),
    ]
}

fn problems(plan: &Plan, facts: &PlanFacts) -> Vec<Problem> {
    PlanValidator::validate(plan, facts)
        .into_iter()
        .filter(|p| p.rule == "profiles" || p.rule == "gates")
        .collect()
}

fn has(problems: &[Problem], rule: &str, task: Option<&str>, needle: &str) -> bool {
    problems
        .iter()
        .any(|p| p.rule == rule && p.task.as_deref() == task && p.message.contains(needle))
}

#[test]
fn a_plan_with_known_sandboxed_profiles_and_both_gates_is_valid() {
    let plan = plan(vec![task("a", "opus"), task("b", "codex-sol")]);
    let facts = facts(good_roster());
    assert_eq!(problems(&plan, &facts), Vec::new());
    assert!(plan_rules_profiles::notes(&plan, &facts).is_empty());
}

#[test]
fn plan_with_an_unknown_profile_is_refused() {
    let plan = plan(vec![task("a", "opus"), task("b", "ghost")]);
    let found = problems(&plan, &facts(good_roster()));
    assert!(
        has(
            &found,
            "profiles",
            Some("b"),
            "`ghost` is not a profile of the roster"
        ),
        "{found:?}"
    );
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn plan_with_a_profile_without_a_fallback_is_refused() {
    let roster = vec![
        profile("opus", None, true),
        profile("codex-sol", Some("ghost"), true),
    ];
    let plan = plan(vec![task("a", "opus"), task("b", "codex-sol")]);
    let found = problems(&plan, &facts(roster));
    assert!(
        has(&found, "profiles", Some("a"), "names no fallback"),
        "{found:?}"
    );
    assert!(
        has(
            &found,
            "profiles",
            Some("b"),
            "fallback `ghost` of profile `codex-sol` is not a profile of the roster"
        ),
        "{found:?}"
    );
}

#[test]
fn plan_with_a_profile_without_sandbox_on_this_os_is_refused() {
    let roster = vec![
        profile("idea-lead", Some("codex-sol"), false),
        profile("codex-sol", Some("idea-lead"), false),
    ];
    let plan = plan(vec![task("a", "idea-lead")]);
    let found = problems(&plan, &facts(roster));
    assert!(
        has(
            &found,
            "profiles",
            Some("a"),
            "not available on this OS: no sandbox"
        ),
        "{found:?}"
    );
}

#[test]
fn plan_whose_profile_has_a_sandboxed_fallback_is_valid_with_a_note() {
    let roster = vec![
        profile("idea-lead", Some("codex-sol"), false),
        profile("codex-sol", Some("idea-lead"), true),
    ];
    let plan = plan(vec![task("a", "idea-lead")]);
    let facts = facts(roster);
    assert_eq!(problems(&plan, &facts), Vec::new());
    let notes = plan_rules_profiles::notes(&plan, &facts);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        has(
            &notes,
            "profiles",
            Some("a"),
            "not available on this OS: no sandbox"
        ),
        "{notes:?}"
    );
    assert!(
        has(&notes, "profiles", Some("a"), "`codex-sol`"),
        "{notes:?}"
    );
}

#[test]
fn plan_without_declared_fast_gate_is_refused() {
    let plan = plan(vec![task("a", "opus")]);
    let mut facts = facts(good_roster());
    facts.gates.fast = None;
    let found = problems(&plan, &facts);
    assert!(has(&found, "gates", None, "no fast gate"), "{found:?}");
    assert_eq!(found.len(), 1, "{found:?}");

    facts.gates.fast = Some("  ".into());
    let found = problems(&plan, &facts);
    assert!(has(&found, "gates", None, "no fast gate"), "{found:?}");
}

#[test]
fn plan_without_declared_full_gate_is_refused() {
    let plan = plan(vec![task("a", "opus")]);
    let mut facts = facts(good_roster());
    facts.gates.full = None;
    let found = problems(&plan, &facts);
    assert!(has(&found, "gates", None, "no full gate"), "{found:?}");
    assert_eq!(found.len(), 1, "{found:?}");
}
