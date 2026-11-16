//! Phase 0 red tests for FM-1 (the orchestrator forgets to create a task)
//! and FM-8 (2 ideas sent back to back). Both become green in Phase 6:
//! the plan validator refuses a plan with an uncovered requirement, and
//! `horch idea add` keeps each idea in its own file and ledger row.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Output, Stdio};

use horch_e2e::expected::check;
use horch_e2e::harness::Harness;
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A harness with its own ledger file and a git repository.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

fn json_of(h: &Harness, args: &[&str]) -> Value {
    let o = h.run(args);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

fn ticket(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.md"));
    std::fs::write(&p, body).unwrap();
    p
}

/// FM-1: a plan where a requirement of the spec has no task is refused.
#[test]
fn fm1_plan_with_an_uncovered_requirement_is_refused() {
    check(
        file!(),
        "fm1_plan_with_an_uncovered_requirement_is_refused",
        || {
            let h = world("p0-fm1");
            let specs = h.project.join("docs/specs");
            std::fs::create_dir_all(&specs).unwrap();
            std::fs::write(
                specs.join("xr.md"),
                "# XR\n\n| ID | Requirement | Check |\n|---|---|---|\n\
             | XR-01 | The tool reads the limit. | A test. |\n\
             | XR-02 | The tool logs each retry. | A test. |\n",
            )
            .unwrap();
            ticket(
                &h,
                "01-limit",
                "# 01: Read the limit\n\n<!-- horch-ticket: 1 -->\n\
             **Persona:** primary `opus` · **Parallel:** yes\n\
             **Kind:** feature\n\
             **Check:** `true`\n\
             **Own:** `src/limit.rs`\n\n\
             Spec: `docs/specs/xr.md`\n\n\
             ## Goal\n\nThe tool reads the limit.\n\n\
             ## Requirements\n\n- **XR-01**: The tool reads the limit.\n\n\
             ## Steps\n\n1. Read the limit.\n\n\
             ## Done when\n\nThe Check passes.\n",
            );
            std::fs::write(
                h.project.join("tickets/MAP.md"),
                "# Map: xr\n\nSpec: `docs/specs/xr.md`\n\n\
             | # | Title | Blocked by | Wave |\n|---|---|---|---|\n\
             | 01 | [Read the limit](01-limit.md) |  | 1 |\n",
            )
            .unwrap();
            let o = h.run(&["ledger", "add-map", "--dry-run", "tickets/MAP.md"]);
            assert!(
                !o.status.success(),
                "add-map --dry-run accepts a plan with no task for XR-02: {}",
                text(&o)
            );
            assert!(
                text(&o).contains("XR-02"),
                "add-map --dry-run refuses the plan but does not name XR-02: {}",
                text(&o)
            );
        },
    );
}

/// FM-8: 2 ideas sent at the same time stay 2 ideas, each in its own folder.
#[test]
fn fm8_two_ideas_sent_back_to_back_stay_2_ideas() {
    check(
        file!(),
        "fm8_two_ideas_sent_back_to_back_stay_2_ideas",
        || {
            let h = world("p0-fm8");
            let texts = [
                "Retry sync with backoff and log each retry",
                "Show the queue length in the status bar",
            ];
            let children: Vec<_> = texts
                .iter()
                .map(|t| {
                    h.horch(&["idea", "add", t])
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                        .unwrap()
                })
                .collect();
            for (t, child) in texts.iter().zip(children) {
                let o = child.wait_with_output().unwrap();
                assert!(o.status.success(), "horch idea add \"{t}\": {}", text(&o));
            }

            let board = json_of(&h, &["ideas", "--json"]);
            let ideas = board["ideas"]
                .as_array()
                .unwrap_or_else(|| panic!("horch ideas --json has no ideas list: {board}"));
            assert_eq!(ideas.len(), 2, "horch ideas --json: {board}");
            let mut folders = Vec::new();
            for idea in ideas {
                let folder = PathBuf::from(
                    idea["folder"]
                        .as_str()
                        .unwrap_or_else(|| panic!("idea has no folder: {idea}")),
                );
                let body = std::fs::read_to_string(folder.join("idea.md"))
                    .unwrap_or_else(|e| panic!("{}: {e}", folder.display()));
                let own: Vec<_> = texts.iter().filter(|t| body.contains(*t)).collect();
                assert_eq!(own.len(), 1, "idea.md holds not exactly 1 idea: {body}");
                folders.push(folder);
            }
            assert_ne!(folders[0], folders[1], "2 ideas share 1 folder: {board}");
        },
    );
}
