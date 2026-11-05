//! TRF-02: the ticket-writer template's example ticket and MAP parse under
//! the C1 rules (`docs/specs/ticket-format.md`) with no error.

use std::path::{Path, PathBuf};

use horch_core::taskledger::map;

fn template() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates/ticket-writer.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The first fenced block after the line `marker`, without its fences.
fn fenced_after(text: &str, marker: &str) -> String {
    let mut lines = text.lines().skip_while(|l| l.trim() != marker);
    assert!(lines.next().is_some(), "the template has no '{marker}' line");
    let mut lines = lines.skip_while(|l| !l.trim_start().starts_with("```"));
    assert!(lines.next().is_some(), "no fenced block after '{marker}'");
    let body: Vec<&str> = lines.take_while(|l| !l.trim_start().starts_with("```")).collect();
    assert!(!body.is_empty(), "the block after '{marker}' is empty");
    body.join("\n") + "\n"
}

/// Write the example MAP into `dir`, and the example ticket at every ticket
/// file the MAP links.
fn lay_out(dir: &Path, ticket: &str, map_text: &str) -> PathBuf {
    let tickets = dir.join("docs/tickets/example");
    std::fs::create_dir_all(&tickets).unwrap();
    let mut linked = 0;
    for (_, target) in map::links(map_text) {
        if target.ends_with(".md") {
            std::fs::write(tickets.join(&target), ticket).unwrap();
            linked += 1;
        }
    }
    assert!(linked >= 2, "the example MAP links {linked} ticket files; want 2 or more");
    let map_path = tickets.join("MAP.md");
    std::fs::write(&map_path, map_text).unwrap();
    map_path
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
    let map_text = fenced_after(&text, "== Example MAP.md ==");
    assert!(
        !map_text.to_lowercase().contains("| areas"),
        "the example MAP has an Areas column"
    );
    let dir = tempfile::tempdir().unwrap();
    let map_path = lay_out(dir.path(), &ticket, &map_text);

    let parsed = map::read(&map_path, dir.path()).expect("the example MAP parses");
    assert!(parsed.spec_ref.as_deref().is_some_and(|s| s.starts_with("docs/specs/")));
    assert!(parsed.items.len() >= 2);
    for item in &parsed.items {
        let t = &item.task;
        assert!(!t.primary.is_empty(), "ticket {} has no primary persona", item.key);
        assert!(t.land_check.is_some(), "ticket {} has no Check", item.key);
        assert!(!t.own.is_empty(), "ticket {} has no Own paths", item.key);
        assert!(t.task_file.starts_with("docs/tickets/"), "{}", t.task_file);
    }
    assert!(
        parsed.items.iter().any(|i| !i.task.blocked_by.is_empty()),
        "the example MAP shows no blocking edge"
    );
}
