//! FR-7: the only bytes that horch types into a pane are fixed lines from 1
//! list, `delivery::TYPED_LINES`. The text of a message stays in its mail
//! file (Q9 of `docs/tickets/fleetd-p5/MAP.md`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use horch_core::execution::SessionMode;
use horch_core::fleetd::task_execution::domain::stall::DOORBELL;
use horch_core::messaging::delivery::{self, TypedLine, MAIL_LINE, TYPED_LINES};
use horch_core::messaging::mail::Mail;
use horch_core::prompts::{agent_prompt, context_message, worker_prompt_with_context};
use horch_core::roster::Roster;
use horch_core::workspace::client::WorkspaceClient;
use horch_core::workspace::testing::FakeWorkspace;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The code of `path` outside tests, with comments and the contents of
/// string and char literals blanked ([`blank_literals`]). A file named
/// `tests.rs` is a test module. A `#[cfg(test)]` item is skipped: a 1-line
/// item to its `;`, a block item to its closing brace. The scan goes on
/// after it.
fn production_code(path: &Path) -> String {
    if path.file_name().is_some_and(|n| n == "tests.rs") {
        return String::new();
    }
    let text = blank_literals(&std::fs::read_to_string(path).unwrap());
    let mut code = String::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            skip_item(&mut lines);
            continue;
        }
        code.push_str(line);
        code.push('\n');
    }
    code
}

/// `text` with each comment and the contents of each string, raw string and
/// char literal replaced by spaces. Line breaks stay, so a brace or a
/// parenthesis that is counted is always code.
fn blank_literals(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
        } else if c == 'r' && matches!(next, Some('"') | Some('#')) && !ident_before(&chars, i) {
            let hashes = chars[i + 1..].iter().take_while(|&&h| h == '#').count();
            if chars.get(i + 1 + hashes) != Some(&'"') {
                out.push(c);
                i += 1;
                continue;
            }
            out.push('"');
            i += 2 + hashes;
            let close: Vec<char> = std::iter::once('"')
                .chain((0..hashes).map(|_| '#'))
                .collect();
            while i < chars.len() && !chars[i..].starts_with(&close) {
                out.push(blank(chars[i]));
                i += 1;
            }
            out.push('"');
            i += close.len();
        } else if c == '"' {
            out.push('"');
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    out.push(' ');
                    i += 1;
                }
                if i < chars.len() {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
            out.push('"');
            i += 1;
        } else if c == '\'' && is_char_literal(&chars, i) {
            // An escape (`'\''`, `'\u{2500}'`) ends at the first quote
            // after its escaped char; a plain char literal is 3 chars long.
            let end = if chars[i + 1] == '\\' {
                (i + 3..chars.len())
                    .find(|&j| chars[j] == '\'')
                    .unwrap_or(chars.len() - 1)
            } else {
                i + 2
            };
            out.push_str("''");
            i = end + 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Whether the `r` at `i` is the end of an identifier, as in `for`.
fn ident_before(chars: &[char], i: usize) -> bool {
    i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_')
}

/// Whether the `'` at `i` opens a char literal, not a lifetime: `'x'` or an
/// escape such as `'\n'` or `'\u{2500}'`.
fn is_char_literal(chars: &[char], i: usize) -> bool {
    match chars.get(i + 1) {
        Some('\\') => true,
        Some(_) => chars.get(i + 2) == Some(&'\''),
        None => false,
    }
}

/// Skip the item that follows a `#[cfg(test)]` line.
fn skip_item<'a>(lines: &mut impl Iterator<Item = &'a str>) {
    let mut depth = 0i32;
    let mut opened = false;
    for line in lines.by_ref() {
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        opened |= line.contains('{');
        let trimmed = line.trim_end();
        if (opened && depth <= 0) || (!opened && trimmed.ends_with(';')) {
            return;
        }
    }
}

#[test]
fn blank_literals_hides_braces_in_comments_and_literals() {
    let code = "fn a() { let s = \"{\"; let r = r#\"}\"#; let c = '{'; // }\n/* { */ }\n";
    let blanked = blank_literals(code);
    assert_eq!(blanked.matches('{').count(), 1, "{blanked}");
    assert_eq!(blanked.matches('}').count(), 1, "{blanked}");
    assert_eq!(blanked.lines().count(), 2);
    assert_eq!(
        blank_literals("fn f<'a>(x: &'a str) {}"),
        "fn f<'a>(x: &'a str) {}"
    );
    assert_eq!(blank_literals("let q = '\\''; {").matches('{').count(), 1);
}

/// The argument text of each call `<call>(...)` in `code`.
fn call_arguments(code: &str, call: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = code;
    while let Some(at) = rest.find(call) {
        let after = &rest[at + call.len()..];
        let mut depth = 1;
        let end = after
            .char_indices()
            .find_map(|(i, c)| {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(i)
            })
            .expect("a call has a closing parenthesis");
        out.push(
            after[..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        );
        rest = &after[end..];
    }
    out
}

/// The 2 calls that type text into a pane, as a method and as a path call.
const TYPING_CALLS: [&str; 4] = [
    ".pane_send_text(",
    ".agent_prompt(",
    "::pane_send_text(",
    "::agent_prompt(",
];

/// The herdr adapter implements the typing calls of the trait. Each other
/// file reaches them only through `delivery`.
fn is_adapter(relative: &str) -> bool {
    relative.starts_with("crates/horch-core/src/workspace/")
}

#[test]
fn only_the_fixed_doorbell_line_is_typed() {
    let root = workspace_root();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).unwrap() {
        let src = entry.unwrap().path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    let mut typed = 0;
    let mut offenders = Vec::new();
    for path in &files {
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if is_adapter(&relative) {
            continue;
        }
        // `prompts::agent_prompt` builds a briefing; it types nothing.
        let code = production_code(path).replace("prompts::agent_prompt(", "prompts::briefing(");
        for call in TYPING_CALLS {
            for args in call_arguments(&code, call) {
                let ok = relative == "crates/horch-core/src/messaging/delivery.rs"
                    && args.ends_with(", line.text()");
                if ok {
                    typed += 1;
                } else {
                    offenders.push(format!("{relative}: {call}{args})"));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "each typed text must be `line.text()` of a `TypedLine` in delivery.rs:\n{}",
        offenders.join("\n")
    );
    assert!(
        typed >= 2,
        "delivery.rs types through agent_prompt and pane_send_text"
    );
}

#[test]
fn the_list_holds_each_typed_line_once_and_each_is_1_line() {
    assert!(TYPED_LINES.contains(&DOORBELL));
    assert!(TYPED_LINES.contains(&MAIL_LINE));
    assert_eq!(MAIL_LINE, "You have new mail. Run horch inbox.");
    for line in TypedLine::ALL {
        assert!(
            TYPED_LINES.contains(&line.text()),
            "{line:?} is not in TYPED_LINES"
        );
    }
    assert_eq!(TypedLine::ALL.len(), TYPED_LINES.len());
    for (line, text) in TypedLine::ALL.iter().zip(TYPED_LINES) {
        assert_eq!(line.text(), text, "{line:?} maps to the wrong entry");
    }
    for (i, line) in TYPED_LINES.iter().enumerate() {
        assert!(!line.contains(['\n', '\r']), "{line:?} has a line break");
        assert!(!line.contains('{'), "{line:?} has a placeholder");
        assert!(
            !TYPED_LINES[i + 1..].contains(line),
            "{line:?} is in the list twice"
        );
    }
}

#[test]
fn message_to_the_orchestrator_types_the_mail_line_and_keeps_the_text_in_mail() {
    let state = tempfile::tempdir().unwrap();
    let ws = FakeWorkspace::new();
    let pane = ws
        .workspace_create("fleet", None, false)
        .unwrap()
        .root_pane_id;
    let mail = Mail::for_role(state.path(), "w1", "orchestrator");
    let text = "[opus-1] DONE: The report is at ai_docs/reports/x.md.";

    delivery::post(&ws, &pane, &mail, "opus-1", text).unwrap();

    let typed: Vec<String> = ws
        .calls()
        .into_iter()
        .filter(|c| c.method == "agent_prompt" || c.method == "pane_send_text")
        .map(|c| c.args[1].clone())
        .collect();
    assert_eq!(typed, [MAIL_LINE], "only the mail line is typed");
    let pending = mail.pending().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].text, text, "the mail keeps the text");
}

/// The == Compact instructions == section of a rendered brief.
fn compact_section(brief: &str) -> String {
    let start = brief
        .find("== Compact instructions ==")
        .expect("the brief has a Compact instructions section");
    let rest = &brief[start..];
    let end = rest[2..].find("\n== ").map_or(rest.len(), |i| i + 2);
    rest[..end].split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A brief to check: (teammate, role, template key, facts).
type Case<'a> = (&'a str, &'a str, &'a str, &'a [(&'a str, &'a str)]);

/// `/compact` takes no argument (FR-7), so each fact of the 2 templates
/// `instructions` and `instructions-orchestrator` must be in the brief.
/// Each pair is (the words of the template, the words of the brief).
#[test]
fn the_briefs_hold_each_fact_of_the_compact_templates() {
    let roster = Roster::builtin().unwrap();
    let cases: [Case; 2] = [
        (
            "opus",
            "opus-7",
            "instructions",
            &[
                ("your role opus-7", "your role, opus-7"),
                (
                    "your plan or brief path",
                    "your task id and your ticket path",
                ),
                (
                    "your handoff file ai_docs/handoffs/opus-7-whats-next.md",
                    "your handoff file, ai_docs/handoffs/opus-7-whats-next.md",
                ),
                (
                    "the files you touched and their state",
                    "every file you touched, and whether it is committed",
                ),
                ("your decisions", "the decisions you made"),
                (
                    "the open questions you sent and their answers",
                    "every open question you sent to the orchestrator, and its answer",
                ),
                ("your report target", "your report target, the orchestrator"),
                (
                    "The full state is in ai_docs/handoffs/opus-7-whats-next.md.",
                    "The full state is in ai_docs/handoffs/opus-7-whats-next.md.",
                ),
            ],
        ),
        (
            "orchestrator",
            "orchestrator",
            "instructions-orchestrator",
            &[
                ("your actual run slug", "the run slug"),
                (
                    "ai_docs/plans/<run-slug>/decisions.md",
                    "ai_docs/plans/<run-slug>/decisions.md",
                ),
                ("your plan file", "your plan file"),
                (
                    "every open question and its answer",
                    "every open question from a worker, and its answer",
                ),
                (
                    "every COMPACT-READY line you did not yet act on",
                    "every COMPACT-READY line you did not yet act on",
                ),
                ("and your decisions", "the decisions you made"),
                (
                    "Use the actual slug and path from your run, not placeholder text.",
                    "Use the actual slug and path from your run, not placeholder text.",
                ),
                (
                    "rebuild the roster and ownership from horch ledger status",
                    "rebuild roster and ownership from horch ledger status",
                ),
                (
                    "keep the live roster and ownership map",
                    "the live roster and ownership map",
                ),
                (
                    "The full state is in ai_docs/handoffs/orchestrator-whats-next.md.",
                    "The full state is in ai_docs/handoffs/orchestrator-whats-next.md.",
                ),
            ],
        ),
    ];
    for (teammate, role, key, facts) in cases {
        let handoff = format!("ai_docs/handoffs/{role}-whats-next.md");
        let vars = BTreeMap::from([("role", role), ("handoff", handoff.as_str())]);
        let template = context_message(&roster, key, &vars).unwrap();
        let seat = roster.require(teammate).unwrap();
        let brief = if role == "orchestrator" {
            agent_prompt(&roster, seat, role)
        } else {
            worker_prompt_with_context(
                &roster,
                seat,
                role,
                "Run horch ledger claim.",
                &SessionMode::Fresh(None),
                "(No project facts recorded.)",
                "test-host",
            )
        }
        .unwrap();
        let section = compact_section(&brief);
        for (said, kept) in facts {
            assert!(
                template.contains(said),
                "{key} no longer says {said:?}: {template}"
            );
            assert!(
                section.contains(kept),
                "the brief of {role} lacks {kept:?}: {section}"
            );
        }
    }
}
