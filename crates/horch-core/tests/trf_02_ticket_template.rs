//! TRF-02: the ticket-writer template's example ticket and plan.yaml parse
//! under the C1 rules (`docs/specs/ticket-format.md`) with no error.

use std::path::Path;

use horch_core::fleetd::ideas::adapters::plan_yaml;
use horch_core::taskledger::map;

fn template() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates/ticket-writer.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The first fenced block after the line `marker`, without its fences.
fn fenced_after(text: &str, marker: &str) -> String {
    let mut lines = text.lines().skip_while(|l| l.trim() != marker);
    assert!(
        lines.next().is_some(),
        "the template has no '{marker}' line"
    );
    let mut lines = lines.skip_while(|l| !l.trim_start().starts_with("```"));
    assert!(lines.next().is_some(), "no fenced block after '{marker}'");
    let body: Vec<&str> = lines
        .take_while(|l| !l.trim_start().starts_with("```"))
        .collect();
    assert!(!body.is_empty(), "the block after '{marker}' is empty");
    body.join("\n") + "\n"
}

#[test]
fn trf_02_template_example_ticket_has_the_c1_field_block() {
    let ticket = fenced_after(&template(), "== Example ticket ==");
    let mut lines = ticket.lines().filter(|l| !l.trim().is_empty());
    assert!(lines.next().unwrap().starts_with("# 01: "));
    let block: Vec<&str> = lines.take(6).collect();
    let want = [
        "<!-- horch-ticket: 1 -->",
        "**Persona:**",
        "**Kind:**",
        "**Review:**",
        "**Check:**",
        "**Own:**",
    ];
    for (line, prefix) in block.iter().zip(want) {
        assert!(line.starts_with(prefix), "want '{prefix}', got '{line}'");
    }

    let facts = map::ticket_facts(&ticket);
    assert!(facts.primary.is_some(), "no primary persona");
    assert!(facts.fallback.is_some(), "no fallback persona");
    assert!(facts.check.is_some(), "no Check command");
    assert!(!facts.own.is_empty(), "no Own paths");
}

#[test]
fn trf_02_template_example_map_parses_with_the_ticket() {
    let text = template();
    let ticket = fenced_after(&text, "== Example ticket ==");
    let plan_text = fenced_after(&text, "== Example plan.yaml ==");
    let dir = tempfile::tempdir().unwrap();
    let named = plan_yaml::parse(&plan_text).expect("the example plan.yaml parses");
    for task in &named.tasks {
        let ticket_path = task.ticket.as_ref().expect("a task names its ticket");
        std::fs::write(dir.path().join(&ticket_path.path), &ticket).unwrap();
    }
    let plan_path = dir.path().join("plan.yaml");
    std::fs::write(&plan_path, &plan_text).unwrap();

    let plan = plan_yaml::read(&plan_path).expect("the example plan.yaml reads");
    assert!(plan.tasks.len() >= 2);
    for t in &plan.tasks {
        assert!(!t.profile.is_empty(), "task {} has no profile", t.key);
        assert!(!t.covers.is_empty(), "task {} covers nothing", t.key);
        assert!(!t.acceptance.is_empty(), "task {} has no acceptance", t.key);
        let file = t.ticket.as_ref().expect("a task names its ticket");
        assert!(
            file.problems.is_empty(),
            "ticket of {} breaks the C1 rules: {:?}",
            t.key,
            file.problems
        );
    }
    assert!(
        plan.tasks.iter().any(|t| !t.after.is_empty()),
        "the example plan shows no blocking edge"
    );
}
