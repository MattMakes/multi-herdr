use std::path::Path;

use horch_core::fleetd::ideas::adapters::plan_yaml;
use horch_core::fleetd::ideas::domain::plan::Plan;
use horch_core::fleetd::ideas::domain::plan_validator::{
    PlanFacts, PlanValidator, Problem, Requirement,
};

const C1_TICKET: &str = "# 01: Add backoff

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`
**Kind:** feature
**Check:** `pytest tests/test_backoff.py -q`
**Own:** `src/sync/client.py`, `tests/test_backoff.py`

## Goal

Retry with backoff.
";

fn plan_text(a_covers: &str, b_covers: &str) -> String {
    format!(
        "idea: I-0001
repo: /fleet/repos/app
feature: F-0001
base_branch: feature/x
tasks:
  - key: a
    title: Task a
    kind: feature
    profile: coder-codex
    covers: [{a_covers}]
    acceptance:
      - run: cargo test
  - key: b
    title: Task b
    kind: feature
    profile: coder-codex
    covers: [{b_covers}]
    after: [a]
    acceptance:
      - run: cargo test
"
    )
}

fn read_plan(dir: &Path, plan: &str, tickets: &[(&str, &str)]) -> Plan {
    for (path, text) in tickets {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let file = dir.join("plan.yaml");
    std::fs::write(&file, plan).unwrap();
    plan_yaml::read(&file).unwrap()
}

fn requirement(id: &str, cites: &[&str]) -> Requirement {
    Requirement {
        id: id.to_string(),
        text: format!("requirement {id}"),
        cites: cites.iter().map(|c| c.to_string()).collect(),
    }
}

fn facts() -> PlanFacts {
    PlanFacts {
        requirements: vec![requirement("R1", &["A1"]), requirement("R2", &["Q1"])],
        approved_assumptions: vec!["A1".into()],
        answers: vec!["Q1".into()],
        ..PlanFacts::default()
    }
}

fn coverage(plan: &Plan, facts: &PlanFacts) -> Vec<Problem> {
    PlanValidator::validate(plan, facts)
        .into_iter()
        .filter(|p| p.rule == "coverage")
        .collect()
}

#[test]
fn a_covered_and_cited_plan_has_no_coverage_problem() {
    let dir = tempfile::tempdir().unwrap();
    let plan = read_plan(dir.path(), &plan_text("R1", "R2"), &[]);
    assert_eq!(coverage(&plan, &facts()), Vec::new());
}

#[test]
fn i3_uncovered_requirement_is_reported_and_plan_refused() {
    let dir = tempfile::tempdir().unwrap();
    let plan = read_plan(dir.path(), &plan_text("R1", "R1"), &[]);
    let problems = coverage(&plan, &facts());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].task, None);
    assert!(problems[0].message.contains("R2"), "{problems:?}");
    assert!(problems[0].message.contains("no task"), "{problems:?}");
    assert!(!PlanValidator::validate(&plan, &facts()).is_empty());
}

#[test]
fn task_covering_an_unknown_requirement_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let plan = read_plan(dir.path(), &plan_text("R1, R9", "R2"), &[]);
    let problems = coverage(&plan, &facts());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].task.as_deref(), Some("a"));
    assert!(problems[0].message.contains("R9"), "{problems:?}");
}

#[test]
fn requirement_with_no_approved_citation_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let plan = read_plan(dir.path(), &plan_text("R1", "R2"), &[]);
    let mut facts = facts();
    facts.requirements = vec![requirement("R1", &[]), requirement("R2", &["A2"])];
    let problems = coverage(&plan, &facts);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems.iter().all(|p| p.task.is_none()));
    assert!(problems.iter().any(|p| p.message.contains("R1")));
    assert!(problems
        .iter()
        .any(|p| p.message.contains("R2") && p.message.contains("A2")));
}

#[test]
fn direct_plan_covers_its_ticket() {
    let dir = tempfile::tempdir().unwrap();
    let ticket = "tickets/01-backoff.md";
    let plan_yaml = format!(
        "idea: I-0002
repo: /fleet/repos/app
feature: F-0001
base_branch: feature/x
tasks:
  - key: backoff
    title: Add backoff
    ticket: {ticket}
    kind: feature
    profile: coder-codex
    covers: [R1]
    acceptance:
      - run: cargo test
"
    );
    let plan = read_plan(dir.path(), &plan_yaml, &[(ticket, C1_TICKET)]);
    let facts = PlanFacts {
        requirements: vec![Requirement {
            id: "R1".into(),
            text: "Add backoff".into(),
            cites: vec![ticket.into()],
        }],
        approved_assumptions: vec![ticket.into()],
        ..PlanFacts::default()
    };
    assert_eq!(coverage(&plan, &facts), Vec::new());

    let uncovered = read_plan(
        dir.path(),
        &plan_yaml.replace("covers: [R1]", "covers: []"),
        &[],
    );
    let problems = coverage(&uncovered, &facts);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].message.contains("R1"));
}
