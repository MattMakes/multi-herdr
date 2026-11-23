use std::path::Path;

use horch_core::fleetd::ideas::adapters::plan_yaml;
use horch_core::fleetd::ideas::domain::plan::{Acceptance, Kind, Plan};
use horch_core::fleetd::ideas::domain::plan_validator::{PlanFacts, PlanValidator, Problem};

const DESIGN_EXAMPLE: &str = r###"idea: I-0007
repo: /fleet/repos/app
feature: F-0003
base_branch: feature/retry-sync-backoff   # the feature branch; every task of the plan lands here
tasks:
  - key: backoff
    title: Add exponential backoff to SyncClient.request
    ticket: tickets/01-backoff.md   # Markdown, today's format C1
    kind: feature
    profile: coder-codex
    covers: [R1]
    inputs: [src/sync/client.py]
    outputs: [src/sync/client.py, tests/test_backoff.py]
    acceptance:
      - run: pytest tests/test_backoff.py -q
      - run: pytest tests/sync -q
    budget: {max_minutes: 40}
  - key: logging
    title: Log each retry with attempt and delay
    kind: feature
    profile: coder-claude
    covers: [R2]
    after: [backoff]        # met when backoff is done and its task branch is landed on the feature branch
    acceptance:
      - run: pytest tests/test_backoff.py -q -k logs
  - key: sync-suite
    title: Run the existing sync suite on the feature branch
    kind: quality
    profile: coder-codex
    covers: [R3]
    after: [backoff, logging]
    acceptance:
      - run: pytest -q
  - key: retry-doc
    title: Document the retry behavior in docs/sync.md
    kind: docs              # a judgment kind: structural check plus a judge
    profile: writer-claude
    covers: [R2]
    outputs: [docs/sync.md]
    acceptance:
      - sections: [docs/sync.md, "## Retries"]   # structural: the file and the section exist
  - key: retry-doc-judge
    title: Judge docs/sync.md against R1 and R2
    kind: judge             # not a judgment kind: it needs no judge of its own
    judges: retry-doc       # plan validation refuses a judgment task with no judge; met when retry-doc is in verifying and its structural check passed
    profile: judge-codex    # a different session, and a different provider from the author
                            # no after: [retry-doc]; the worktree starts from the tip of the retry-doc task branch, and retry-doc is done only after this verdict
    outputs: [docs/sync.review.md]   # each miss ranked CRITICAL, HIGH, MEDIUM or LOW
"###;

const C1_TICKET: &str = "# 01: Add backoff

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`
**Kind:** feature
**Check:** `pytest tests/test_backoff.py -q`
**Own:** `src/sync/client.py`, `tests/test_backoff.py`

## Goal

Retry with backoff.
";

fn plan_text(extra: &str) -> String {
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
    covers: [R1]
    acceptance:
      - run: cargo test
  - key: b
    title: Task b
    kind: feature
    profile: coder-codex
    covers: [R1]
    after: [a]
    acceptance:
      - run: cargo test
{extra}"
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

fn frame_problems(plan: &Plan) -> Vec<Problem> {
    PlanValidator::validate(plan, &PlanFacts::default())
        .into_iter()
        .filter(|p| p.rule == "schema" || p.rule == "dag")
        .collect()
}

fn has(problems: &[Problem], rule: &str, task: &str, needle: &str) -> bool {
    problems
        .iter()
        .any(|p| p.rule == rule && p.task.as_deref() == Some(task) && p.message.contains(needle))
}

#[test]
fn a_valid_plan_has_no_problem() {
    let dir = tempfile::tempdir().unwrap();
    let plan = read_plan(dir.path(), &plan_text(""), &[]);
    assert_eq!(frame_problems(&plan), Vec::new());
}

#[test]
fn plan_with_a_dependency_cycle_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = plan_text(
        "  - key: c
    title: Task c
    kind: feature
    profile: coder-codex
    after: [b]
    acceptance:
      - run: cargo test
  - key: d
    title: Task d
    kind: feature
    profile: coder-codex
    after: [c]
    acceptance:
      - run: cargo test
",
    )
    .replace(
        "    covers: [R1]\n    acceptance:\n      - run: cargo test\n  - key: b",
        "    covers: [R1]\n    after: [c]\n    acceptance:\n      - run: cargo test\n  - key: b",
    );
    let plan = read_plan(dir.path(), &text, &[]);
    let problems = frame_problems(&plan);
    let cycles: Vec<&Problem> = problems.iter().filter(|p| p.rule == "dag").collect();
    assert_eq!(cycles.len(), 1, "{problems:?}");
    assert!(
        cycles[0].message.contains("a -> c -> b -> a")
            || cycles[0].message.contains("a -> b -> c -> a"),
        "{problems:?}"
    );
    assert!(!problems.iter().any(|p| p.task.as_deref() == Some("d")));
}

#[test]
fn a_task_after_itself_is_a_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let text = plan_text("").replace("after: [a]", "after: [b]");
    let plan = read_plan(dir.path(), &text, &[]);
    assert!(has(&frame_problems(&plan), "dag", "b", "cycle"));
}

#[test]
fn plan_with_an_unknown_after_key_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = plan_text("").replace("after: [a]", "after: [a, nope]");
    let plan = read_plan(dir.path(), &text, &[]);
    assert!(has(&frame_problems(&plan), "dag", "b", "`nope`"));
}

#[test]
fn task_without_an_acceptance_check_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = plan_text(
        "  - key: c
    title: Task c
    kind: feature
    profile: coder-codex
  - key: d
    title: Task d
    kind: docs
    profile: coder-codex
    acceptance: []
  - key: e
    title: Task e
    kind: feature
    profile: coder-codex
    acceptance:
      - run: \"  \"
",
    );
    let plan = read_plan(dir.path(), &text, &[]);
    let problems = frame_problems(&plan);
    assert!(has(&problems, "schema", "c", "acceptance"), "{problems:?}");
    assert!(has(&problems, "schema", "d", "acceptance"), "{problems:?}");
    assert!(has(&problems, "schema", "e", "acceptance"), "{problems:?}");
}

#[test]
fn task_with_an_unknown_kind_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = plan_text("").replacen("kind: feature", "kind: chore", 1);
    let plan = read_plan(dir.path(), &text, &[]);
    assert_eq!(plan.tasks[0].kind, None);
    assert!(has(&frame_problems(&plan), "schema", "a", "`chore`"));
}

#[test]
fn task_without_a_profile_or_with_a_duplicate_key_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = plan_text(
        "  - key: a
    title: Again
    kind: feature
    acceptance:
      - run: cargo test
",
    );
    let plan = read_plan(dir.path(), &text, &[]);
    let problems = frame_problems(&plan);
    assert!(has(&problems, "schema", "a", "profile"), "{problems:?}");
    assert!(has(&problems, "schema", "a", "2 tasks"), "{problems:?}");
}

#[test]
fn task_whose_ticket_is_not_c1_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = plan_text("")
        .replacen(
            "    kind: feature",
            "    ticket: tickets/a.md\n    kind: feature",
            1,
        )
        .replacen(
            "  - key: b\n    title: Task b\n",
            "  - key: b\n    title: Task b\n    ticket: tickets/missing.md\n",
            1,
        );
    let plan = read_plan(
        dir.path(),
        &text,
        &[("tickets/a.md", "# A ticket\n\nNo field block.\n")],
    );
    let problems = frame_problems(&plan);
    assert!(
        has(&problems, "schema", "a", "tickets/a.md"),
        "{problems:?}"
    );
    assert!(
        has(&problems, "schema", "b", "tickets/missing.md"),
        "{problems:?}"
    );

    let ok = read_plan(dir.path(), &text, &[("tickets/a.md", C1_TICKET)]);
    assert!(!has(&frame_problems(&ok), "schema", "a", "tickets/a.md"));
}

#[test]
fn a_ticket_path_outside_the_plan_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = plan_text("").replacen(
        "    kind: feature",
        "    ticket: ../outside.md\n    kind: feature",
        1,
    );
    std::fs::write(dir.path().join("outside.md"), C1_TICKET).unwrap();
    let inner = dir.path().join("idea");
    std::fs::create_dir(&inner).unwrap();
    let plan = read_plan(&inner, &text, &[]);
    assert!(has(&frame_problems(&plan), "schema", "a", "../outside.md"));
}

#[test]
fn a_plan_with_no_task_or_no_idea_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let plan = read_plan(
        dir.path(),
        "idea: \"\"\nrepo: /r\nfeature: F-0001\nbase_branch: feature/x\ntasks: []\n",
        &[],
    );
    let problems = frame_problems(&plan);
    assert!(problems
        .iter()
        .any(|p| p.task.is_none() && p.message.contains("idea")));
    assert!(problems
        .iter()
        .any(|p| p.task.is_none() && p.message.contains("no task")));
}

#[test]
fn an_unknown_field_is_a_read_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plan.yaml");
    std::fs::write(&file, plan_text("").replace("covers: [R1]", "cover: [R1]")).unwrap();
    let err = plan_yaml::read(&file).unwrap_err();
    assert!(format!("{err:#}").contains("cover"), "{err:#}");
}

#[test]
fn the_example_plan_of_the_design_parses() {
    let dir = tempfile::tempdir().unwrap();
    let plan = read_plan(
        dir.path(),
        DESIGN_EXAMPLE,
        &[("tickets/01-backoff.md", C1_TICKET)],
    );
    assert_eq!(plan.idea, "I-0007");
    assert_eq!(plan.repo, "/fleet/repos/app");
    assert_eq!(plan.feature, "F-0003");
    assert_eq!(plan.base_branch, "feature/retry-sync-backoff");
    let keys: Vec<&str> = plan.tasks.iter().map(|t| t.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "backoff",
            "logging",
            "sync-suite",
            "retry-doc",
            "retry-doc-judge"
        ]
    );
    let backoff = &plan.tasks[0];
    assert_eq!(backoff.kind, Some(Kind::Feature));
    assert_eq!(backoff.budget.as_ref().unwrap().max_minutes, Some(40));
    assert_eq!(
        backoff.ticket.as_ref().unwrap().path,
        "tickets/01-backoff.md"
    );
    assert_eq!(
        backoff.acceptance[1],
        Acceptance::Run("pytest tests/sync -q".into())
    );
    assert_eq!(plan.tasks[2].after, ["backoff", "logging"]);
    let doc = &plan.tasks[3];
    assert!(doc.kind.unwrap().is_judgment());
    assert_eq!(
        doc.acceptance,
        [Acceptance::Sections {
            file: "docs/sync.md".into(),
            sections: vec!["## Retries".into()],
        }]
    );
    let judge = &plan.tasks[4];
    assert_eq!(judge.kind, Some(Kind::Judge));
    assert_eq!(judge.judges.as_deref(), Some("retry-doc"));
    assert_eq!(frame_problems(&plan), Vec::new());
}

#[test]
fn an_output_path_outside_the_repository_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for (path, needle) in [
        ("/etc/passwd", "not a relative path"),
        ("docs/../../x.md", "`..`"),
        ("C:/x.md", "not a relative path"),
        (" ", "is empty"),
    ] {
        let mut plan = read_plan(dir.path(), &plan_text(""), &[]);
        plan.tasks[0].outputs = vec![path.to_string()];
        let problems = frame_problems(&plan);
        assert!(
            has(&problems, "schema", "a", needle),
            "{path}: {problems:?}"
        );
    }
    let mut plan = read_plan(dir.path(), &plan_text(""), &[]);
    plan.tasks[0].outputs = vec!["docs/sync.review.md".into()];
    assert_eq!(frame_problems(&plan), Vec::new());
}

#[test]
fn two_tasks_with_the_same_output_path_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut plan = read_plan(dir.path(), &plan_text(""), &[]);
    plan.tasks[0].outputs = vec!["docs/sync.review.md".into()];
    plan.tasks[1].outputs = vec!["docs/sync.review.md".into()];
    let problems = frame_problems(&plan);
    assert!(
        has(&problems, "schema", "a", "2 tasks have the output"),
        "{problems:?}"
    );
    assert_eq!(problems.len(), 1, "1 problem for the path: {problems:?}");
}
