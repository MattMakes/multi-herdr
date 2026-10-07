//! The fleet pi compaction extension (`assets/pi/horch-compact.ts`, X3):
//! its `session_before_compact` handler, run with `node` on a fixture
//! branch. No pi and no model: the handler is all that pi calls.
//!
//! `node` (22.6 or later strips the TypeScript types) runs the asset; a
//! machine without `node` skips these tests with a line on stderr.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

const HANDOFF: &str = "ai_docs/handoffs/pi-3-whats-next.md";
const KEEP: &str = "Keep in the summary: your role pi-3, your plan or brief path, your handoff file ai_docs/handoffs/pi-3-whats-next.md, the files you touched and their state, your decisions, the open questions you sent and their answers, and your report target. The full state is in ai_docs/handoffs/pi-3-whats-next.md.";

fn asset() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/pi/horch-compact.ts")
}

/// The driver: load the extension with a stub `pi`, call the handler that
/// it registers for `session_before_compact`, print the result as JSON.
/// `fileOps` arrive as arrays and become the `Set`s that pi passes.
const DRIVER: &str = r#"
import { readFileSync } from "node:fs";
const ext = (await import(process.argv[2])).default;
const handlers = {};
ext({ on: (name, fn) => { (handlers[name] ??= []).push(fn); } });
const event = JSON.parse(readFileSync(process.argv[3], "utf8"));
const ops = event.preparation?.fileOps;
if (ops) event.preparation.fileOps = { read: new Set(ops.read), written: new Set(ops.written), edited: new Set(ops.edited) };
const names = Object.keys(handlers).sort();
const out = [];
for (const fn of handlers.session_before_compact ?? []) out.push(await fn(event, {}));
console.log(JSON.stringify({ names, out }));
"#;

fn node() -> Option<()> {
    let ok = Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("SKIP: node is not on PATH");
    }
    ok.then_some(())
}

/// Run the handler on `event`; returns `{ names, out }`.
fn run(event: &Value) -> Value {
    let tmp = tempfile::tempdir().unwrap();
    // `.mts`: an ES module whatever package.json is above it.
    let ext = tmp.path().join("horch-compact.mts");
    std::fs::copy(asset(), &ext).unwrap();
    let driver = tmp.path().join("driver.mjs");
    std::fs::write(&driver, DRIVER).unwrap();
    let input = tmp.path().join("event.json");
    std::fs::write(&input, serde_json::to_vec(event).unwrap()).unwrap();
    let out = Command::new("node")
        .arg("--no-warnings")
        .arg(&driver)
        .arg(&ext)
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "node failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn text(t: &str) -> Value {
    json!({ "type": "text", "text": t })
}

fn bash(id: &str, command: &str) -> Value {
    json!({ "role": "assistant", "content": [
        { "type": "thinking", "thinking": "THINKING-CANARY private reasoning" },
        { "type": "toolCall", "id": id, "name": "bash", "arguments": { "command": command } }
    ]})
}

fn result(id: &str, name: &str, out: &str, is_error: bool) -> Value {
    json!({ "role": "toolResult", "toolCallId": id, "toolName": name,
            "content": [text(out)], "isError": is_error })
}

/// A worker session: the briefing, a read with a long output, an edit, a
/// failed test run, a commit, the handoff note, and an orchestrator line.
fn messages() -> Vec<Value> {
    let long: String = (0..400)
        .map(|i| format!("README line {i} LONG-OUTPUT-MIDDLE\n"))
        .collect();
    vec![
        json!({ "role": "user", "content": [text(
            "You are worker 'pi-3' in a herdr multi-agent fleet workspace.\n\
             Your first assigned task:\n\nX9: read and do ai_docs/plans/x9.md")] }),
        json!({ "role": "assistant", "content": [
            { "type": "thinking", "thinking": "THINKING-CANARY plan" },
            text("I read the plan first."),
            { "type": "toolCall", "id": "c1", "name": "read", "arguments": { "path": "README.md" } }
        ]}),
        result(
            "c1",
            "read",
            &format!("README head\n{long}README tail"),
            false,
        ),
        json!({ "role": "assistant", "content": [
            { "type": "toolCall", "id": "c2", "name": "edit",
              "arguments": { "path": "src/lib.rs", "oldText": "a", "newText": "b" } }
        ]}),
        result("c2", "edit", "Edited src/lib.rs", false),
        bash("c3", "cargo test -p horch-core"),
        result("c3", "bash", "test result: FAILED. 1 failed", true),
        bash("c4", "git commit -m \"Fix the parser\" -- src/lib.rs"),
        result(
            "c4",
            "bash",
            "[design-skills abc1234] Fix the parser",
            false,
        ),
        bash("c5", &format!("horch note \"handoff: {HANDOFF}\"")),
        result("c5", "bash", "", false),
        json!({ "role": "user", "content": "[orchestrator] Use option 2 for the parser." }),
        json!({ "role": "assistant", "content": [text("DECISION-CANARY: I use option 2.")] }),
    ]
}

fn event(instructions: Option<&str>) -> Value {
    json!({
        "type": "session_before_compact",
        "reason": "manual",
        "willRetry": false,
        "customInstructions": instructions,
        "branchEntries": [],
        "preparation": {
            "firstKeptEntryId": "e-kept-17",
            "messagesToSummarize": messages(),
            "turnPrefixMessages": [],
            "isSplitTurn": false,
            "tokensBefore": 31234,
            "previousSummary": "PREVIOUS-SUMMARY-CANARY: the plan is x9.",
            "fileOps": { "read": ["README.md"], "written": [], "edited": ["src/lib.rs"] },
            "settings": { "enabled": true, "reserveTokens": 16384, "keepRecentTokens": 20000 }
        }
    })
}

fn compaction(out: &Value) -> &Value {
    &out["out"][0]["compaction"]
}

#[test]
fn x3_registers_only_the_compaction_handler() {
    let Some(()) = node() else { return };
    let out = run(&event(Some(KEEP)));
    assert_eq!(out["names"], json!(["session_before_compact"]));
}

#[test]
fn x3_summary_keeps_pi_cut_and_says_it_is_horch() {
    let Some(()) = node() else { return };
    let out = run(&event(Some(KEEP)));
    let c = compaction(&out);
    assert_eq!(c["firstKeptEntryId"], "e-kept-17");
    assert_eq!(c["tokensBefore"], 31234);
    assert_eq!(c["details"]["compactor"], "horch");
    assert_eq!(c["details"]["version"], 1);
}

#[test]
fn x3_summary_keeps_the_keep_list_verbatim_and_names_the_handoff() {
    let Some(()) = node() else { return };
    let out = run(&event(Some(KEEP)));
    let s = compaction(&out)["summary"].as_str().unwrap();
    assert!(s.contains(KEEP), "{s}");
    assert!(
        s.contains(&format!("Read {HANDOFF} first.")),
        "the handoff line: {s}"
    );
}

#[test]
fn x3_handoff_comes_from_the_note_when_no_instructions() {
    let Some(()) = node() else { return };
    let out = run(&event(None));
    let s = compaction(&out)["summary"].as_str().unwrap();
    assert!(s.contains(&format!("Read {HANDOFF} first.")), "{s}");
    assert!(!s.contains("[Keep]"), "no argument, no Keep section: {s}");
}

#[test]
fn x3_summary_extracts_goal_files_commits_commands_and_previous() {
    let Some(()) = node() else { return };
    let out = run(&event(Some(KEEP)));
    let s = compaction(&out)["summary"].as_str().unwrap();
    for want in [
        "[Goal]",
        "X9: read and do ai_docs/plans/x9.md",
        "[orchestrator] Use option 2 for the parser.",
        "[Files]",
        "read: README.md",
        "modified: src/lib.rs",
        "[Commits]",
        "git commit -m \"Fix the parser\" -- src/lib.rs",
        "[Commands]",
        "cargo test -p horch-core (error)",
        "[Previous summary]",
        "PREVIOUS-SUMMARY-CANARY",
        "[Transcript]",
        "DECISION-CANARY: I use option 2.",
    ] {
        assert!(s.contains(want), "missing {want:?} in:\n{s}");
    }
}

#[test]
fn x3_tool_output_is_trimmed_and_thinking_dropped() {
    let Some(()) = node() else { return };
    let out = run(&event(Some(KEEP)));
    let s = compaction(&out)["summary"].as_str().unwrap();
    assert!(!s.contains("THINKING-CANARY"), "{s}");
    assert!(s.contains("README head"), "the head of a tool output stays");
    assert!(s.contains("README tail"), "the tail of a tool output stays");
    assert!(
        s.matches("LONG-OUTPUT-MIDDLE").count() < 10,
        "the middle of a long output is trimmed: {s}"
    );
    assert!(s.contains("[trimmed"), "{s}");
}

#[test]
fn x3_summary_is_capped() {
    let Some(()) = node() else { return };
    let mut e = event(Some(KEEP));
    let big: Vec<Value> = (0..2000)
        .map(|i| json!({ "role": "assistant", "content": [text(&format!("step {i} {}", "x".repeat(500)))] }))
        .collect();
    e["preparation"]["messagesToSummarize"] = Value::Array(big);
    e["preparation"]["previousSummary"] = json!("p".repeat(50_000));
    let out = run(&e);
    let s = compaction(&out)["summary"].as_str().unwrap();
    assert!(s.chars().count() <= 16_000, "{} chars", s.chars().count());
    assert!(s.contains(KEEP), "the keep-list survives the cap");
    assert!(s.contains("step 1999"), "the newest step survives the cap");
}

#[test]
fn x3_a_broken_event_falls_back_to_pi() {
    let Some(()) = node() else { return };
    let mut e = event(Some(KEEP));
    e["preparation"] = json!(null);
    let out = run(&e);
    assert_eq!(
        out["out"][0],
        Value::Null,
        "the handler returns nothing, so pi makes its own summary"
    );
}
