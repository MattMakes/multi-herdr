//! `**Check:** none` in a ticket stores "no check", and never the shell
//! command `none`. `horch ledger add` and `horch ledger run new` are retired
//! (FR-12: every task belongs to an idea), so the test reads the ticket files
//! with the map reader that fleetd uses and queues each task with
//! `tasks::add` on a temporary ledger.

use std::path::Path;

use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, Task};
use horch_core::taskledger::{map, schema, util, Store};

fn ticket(dir: &Path, name: &str, check: &str) {
    let text = format!(
        "# 01: {name}\n\n<!-- horch-ticket: 1 -->\n**Persona:** primary `dev`\n**Kind:** quality\n**Check:** {check}\n**Own:** `src/a.rs`\n\n## Goal\n\nDo {name}.\n"
    );
    std::fs::write(dir.join(name), text).unwrap();
}

#[test]
fn check_none_in_ticket_stores_no_check() {
    let dir = tempfile::tempdir().unwrap();
    let tickets = dir.path().join("tickets");
    std::fs::create_dir_all(&tickets).unwrap();
    ticket(&tickets, "a.md", "none");
    ticket(&tickets, "b.md", "`cargo test`");
    std::fs::write(
        tickets.join("MAP.md"),
        "# Map\n\n| # | Title | Blocked by | Wave |\n|---|---|---|---|\n| 01 | [A](a.md) | | 1 |\n| 02 | [B](b.md) | | 1 |\n",
    )
    .unwrap();

    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let run = store
        .immediate(|tx| {
            let new_run = NewRun {
                slug: "r1".into(),
                ..Default::default()
            };
            let run = runs::create(tx, dir.path(), &new_run)?;
            // FR-12: a task needs an idea in flight of the feature of its run.
            schema::run_idea(tx, &run.run_id, &util::now(), true)?;
            Ok(run)
        })
        .unwrap();
    let items = map::read(&tickets.join("MAP.md"), dir.path())
        .unwrap()
        .items;
    let tasks: Vec<Task> = store
        .immediate(|tx| {
            items
                .iter()
                .map(|item| {
                    let queued = tasks::add(tx, &run, &item.task, Actor::Orchestrator)?;
                    tasks::get(tx, &queued.task_id)
                })
                .collect()
        })
        .unwrap();

    assert_eq!(tasks.len(), 2);
    assert!(tasks[0].check_none, "{:?}", tasks[0]);
    assert_eq!(tasks[0].land_check, None, "none is not a command");
    assert!(!tasks[1].check_none, "{:?}", tasks[1]);
    assert_eq!(tasks[1].land_check.as_deref(), Some("cargo test"));
}
