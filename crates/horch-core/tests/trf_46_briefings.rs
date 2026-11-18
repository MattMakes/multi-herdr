//! TRF-46: the briefings describe pull dispatch only, and every non-ok
//! `horch context --over` state has an instruction in them.
//!
//! Pull is the only dispatch model (trace-fixes D1): `horch assign` and
//! task-text spawns are gone, so no briefing may tell an agent to use them.
//! `horch context --over` prints 1 instruction id per state it shows
//! (TRF-08); each id must name an instruction in the orchestrator briefing
//! and in the worker briefing.

use horch_core::compaction::policy::RowState;
use horch_core::execution::SessionMode;
use horch_core::prompts;
use horch_core::roster::Roster;

fn roster() -> Roster {
    Roster::builtin().expect("built-in roster parses")
}

fn orchestrators(r: &Roster) -> Vec<(String, String)> {
    ["orchestrator", "orchestrator-codex"]
        .into_iter()
        .map(|name| {
            let t = r.require(name).unwrap();
            let p = prompts::agent_prompt(r, t, "orchestrator").unwrap();
            (name.to_string(), p)
        })
        .collect()
}

fn workers(r: &Roster) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for name in ["sonnet", "opus", "codex-sol", "codex-terra", "smoke"] {
        let t = r.require(name).unwrap();
        for (label, task, session) in [
            ("task", "Run horch ledger claim.", SessionMode::Fresh(None)),
            ("idle", "", SessionMode::Fresh(None)),
            (
                "resume",
                "Run horch ledger claim.",
                SessionMode::Resume("s-1".parse().unwrap()),
            ),
            ("push", PUSH_LINE, SessionMode::Fresh(None)),
            (
                "push-resume",
                PUSH_LINE,
                SessionMode::Resume("s-1".parse().unwrap()),
            ),
        ] {
            let p = prompts::worker_prompt(r, t, "w-1", task, &session).unwrap();
            out.push((format!("{name}-{label}"), p));
        }
    }
    out
}

/// The first line of the pane of a push attempt (Q8 of
/// `docs/tickets/fleetd-p4/MAP.md`).
const PUSH_LINE: &str = "Read /fleet/tasks/T-0001-01/attempts/A1/inbox/START.md and continue.";

/// The section of `text` that starts with the heading `heading`, up to the
/// next `== ` heading.
fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let at = text
        .find(heading)
        .unwrap_or_else(|| panic!("no section {heading:?}"));
    let rest = &text[at + heading.len()..];
    let end = rest.find("\n== ").map_or(rest.len(), |i| i + 1);
    &rest[..end]
}

fn repo_file(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The push phrases a briefing must not hold, with the line they are on.
fn push_phrases(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        for phrase in ["horch assign", "Read and follow", "with the task text"] {
            if line.contains(phrase) {
                found.push(format!("{phrase:?} in: {line}"));
            }
        }
        // `horch spawn <persona> "<task>"`: a spawn that carries task text.
        let mut rest = line;
        while let Some(at) = rest.find("horch spawn") {
            let after = &rest[at + "horch spawn".len()..];
            let cmd = after.split('`').next().unwrap_or("");
            if cmd.contains('"') || cmd.contains('\'') {
                found.push(format!("a task-text spawn in: {line}"));
            }
            rest = after;
        }
    }
    found
}

#[test]
fn trf_46_briefings_contain_no_push_dispatch() {
    let r = roster();
    let mut texts = orchestrators(&r);
    texts.extend(workers(&r));
    texts.push((
        "skills/orchestrate/SKILL.md".into(),
        repo_file("skills/orchestrate/SKILL.md"),
    ));
    for (name, text) in &texts {
        let found = push_phrases(text);
        assert!(found.is_empty(), "{name}:\n{}", found.join("\n"));
    }
}

#[test]
fn trf_46_the_push_check_finds_each_push_phrase() {
    for line in [
        "  horch assign sonnet-1 \"do it\"",
        "Spawn it with \"Read and follow <path> exactly.\"",
        "Spawn it with the task text \"x\"",
        "run `horch spawn opus \"fix it\"` now",
        "run horch spawn opus-creative 'seat A'",
    ] {
        assert!(!push_phrases(line).is_empty(), "{line}");
    }
    for line in [
        "Run next: horch spawn opus --run solo-w1",
        "`horch spawn <persona>`. Then \"end your turn\".",
    ] {
        assert!(push_phrases(line).is_empty(), "{line}");
    }
}

#[test]
fn trf_46_every_context_state_has_an_instruction() {
    let r = roster();
    let ids: Vec<&str> = RowState::ALL
        .iter()
        .filter_map(|s| s.instruction_id())
        .collect();
    assert_eq!(
        ids.len(),
        RowState::ALL.len() - 1,
        "every state but ok has an id"
    );
    assert!(ids.contains(&"ctx-handoff"));
    for (name, text) in orchestrators(&r).into_iter().chain(workers(&r)) {
        let named = instructed_ids(&text);
        for id in &ids {
            assert!(named.contains(id), "{name} has no instruction for {id}");
        }
    }
}

/// The ids of the instruction lines `- <id>[, <id>...]: <what to do>`.
fn instructed_ids(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|l| {
            l.trim_start()
                .strip_prefix("- ctx-")
                .map(|_| l.trim_start())
        })
        .filter_map(|l| l[2..].split_once(':'))
        .flat_map(|(ids, _)| ids.split(", ").map(str::trim))
        .collect()
}

/// FR-5 and FM-6, the text side: the push section tells a worker to read
/// `START.md`, to commit in the tree it names and to end with `horch result`
/// or a QUESTION: line. It never tells it to search for work, to make a
/// worktree or to run the finish steps. The claim section stays, marked for
/// an attempt that began with a claim.
#[test]
fn the_push_attempt_section_names_start_md_and_horch_result() {
    let r = roster();
    let pushes: Vec<_> = workers(&r)
        .into_iter()
        .filter(|(name, _)| name.contains("-push"))
        .collect();
    assert_eq!(pushes.len(), 10);
    for (name, text) in &pushes {
        assert!(text.contains(PUSH_LINE), "{name}");
        let push = section(text, "== Your task: a push attempt ==");
        for needed in [
            "Read START.md in full",
            "the worktree that START.md names",
            "Do not search for work.",
            "horch result --status completed --summary",
            "send QUESTION:",
            "End your turn only after horch result or a QUESTION: line.",
            "fleetd then runs the finish steps",
        ] {
            assert!(push.contains(needed), "{name}: {needed:?} missing");
        }
        for instruction in [
            "- horch ledger claim",
            "- horch ledger worktree new",
            "- horch ledger finish",
            "Run horch ledger claim",
            "run: horch ledger finish",
        ] {
            assert!(!push.contains(instruction), "{name}: {instruction:?}");
        }
        let claim = section(
            text,
            "== Task ledger: an attempt that began with a claim ==",
        );
        assert!(
            claim.starts_with("\nThis section is only for an attempt that began with a claim"),
            "{name}"
        );
        let worktrees = section(text, "== Worktrees ==");
        assert!(
            worktrees.contains("In a push attempt, START.md names your worktree."),
            "{name}"
        );
    }
}
