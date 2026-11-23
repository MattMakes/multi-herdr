#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, Store};
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use serde_json::Value;

const FLEET_TOML: &str = "pass_period_s = 1\n";

const IDEA_TEXT: &str = "Show the queue length in the status bar";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

struct World {
    h: Harness,
}

impl World {
    fn new(name: &str, workspace: bool) -> World {
        let w = World::fleet(name, workspace, |_| {});
        let o = w.h.run(&["idea", "add", IDEA_TEXT]);
        assert!(o.status.success(), "horch idea add: {}", text(&o));
        w
    }

    fn fleet(name: &str, workspace: bool, seed: impl FnOnce(&Harness)) -> World {
        let mut h = Harness::new(name).with_git();
        let fleet = h.root.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
        h.set(
            "HORCH_LEDGER_DB",
            h.root.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        h.set("HORCH_FAKE_SCENARIO", "exec,stay");
        if workspace {
            h.fleet_workspace();
        }
        seed(&h);
        h.serve_fleetd();
        World { h }
    }

    fn fleet_root(&self) -> PathBuf {
        self.h.fleet_root()
    }

    fn project_label(&self) -> String {
        self.h
            .project
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = rusqlite::Connection::open(self.h.ledger_db()).unwrap();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn diagnostics(&self) -> String {
        let mut calls: Vec<String> = self
            .h
            .calls()
            .iter()
            .map(|c| format!("{} {}", c["fake"], c["argv"]))
            .collect();
        calls.dedup();
        let cut = calls.len().saturating_sub(60);
        calls.drain(..cut);
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
        format!(
            "calls:\n{}\nfleetd.stderr:\n{}\nlaunch.log:\n{}\nattempts:\n{:?}\nideas:\n{:?}",
            calls.join("\n"),
            read(self.h.root.join("fleetd.stderr")),
            read(self.fleet_root().join("run/launch.log")),
            self.rows(
                "SELECT id || ' ' || IFNULL(task_id, '-') || ' ' || state || ' ' \
                 || IFNULL(outcome, '-') FROM attempts"
            ),
            self.rows("SELECT id || ' ' || state FROM ideas"),
        )
    }
}

fn claude_call(w: &World, needle: &str) -> Vec<String> {
    wait_for_or(
        "the fake idea-lead to start",
        || {
            w.h.calls_of("claude").into_iter().find_map(|c| {
                let argv: Vec<String> = c["argv"]
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
                argv.iter().any(|a| a.contains(needle)).then_some(argv)
            })
        },
        || w.diagnostics(),
    )
}

#[test]
fn fleetd_starts_the_fake_idea_lead_of_a_new_idea() {
    let w = World::new("p6-lead", true);

    let idea = wait_for_or(
        "the idea file of the new idea",
        || {
            w.rows("SELECT id FROM ideas").into_iter().find(|id| {
                let file = w.fleet_root().join("ideas").join(id).join("idea.md");
                std::fs::read_to_string(file).is_ok_and(|t| t.contains(IDEA_TEXT))
            })
        },
        || w.diagnostics(),
    );
    let idea = &idea;
    let argv = claude_call(&w, &format!("idea-lead of idea {idea} "));
    let folder = w.fleet_root().join("ideas").join(idea);
    let brief = argv
        .iter()
        .find(|a| a.contains("idea-lead of idea"))
        .unwrap();
    assert!(brief.contains(idea.as_str()), "{brief}");
    assert!(brief.contains("phase clarifying"), "{brief}");
    let word = folder.display().to_string();
    assert!(brief.split_whitespace().any(|w| w == word), "{brief}");
    assert!(
        argv.iter().any(|a| a.contains("Bash(horch idea approve*)")),
        "{argv:?}"
    );
    let attempts = w.rows(&format!(
        "SELECT id FROM attempts WHERE task_id IS NULL AND idea_id = '{idea}' ORDER BY rowid"
    ));
    assert_eq!(
        attempts.first(),
        Some(&format!("{idea}/clarifying/1")),
        "{}",
        w.diagnostics()
    );
}

#[test]
fn an_idea_lead_with_no_pane_and_no_project_workspace_does_not_start() {
    let w = World::new("p6-nopane", false);
    let label = w.project_label();
    let line = wait_for_or(
        "the launch to fail with no pane",
        || {
            let err = std::fs::read_to_string(w.h.root.join("fleetd.stderr")).ok()?;
            err.lines()
                .find(|l| l.contains("has no pane"))
                .map(str::to_string)
        },
        || w.diagnostics(),
    );
    assert!(line.contains("the launch of attempt I-"), "{line}");
    assert!(
        line.contains(&format!("no herdr workspace has the label {label}")),
        "{line}"
    );
    assert!(
        w.h.calls_of("claude")
            .iter()
            .all(|c| !c.to_string().contains("idea-lead of idea")),
        "{}",
        w.diagnostics()
    );
}

fn declare_gates(h: &Harness) {
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    for args in [
        &["add", "fleet.toml"][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
    ] {
        let o = h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
    }
}

fn seed_task(h: &Harness) {
    declare_gates(h);
    let ticket = "# T0\nDo the work of T0.\n";
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t0.md"), ticket).unwrap();
    let store = Store::open(&h.ledger_db()).unwrap();
    let conn = store.conn();
    let run = runs::create(
        conn,
        &h.project,
        &NewRun {
            slug: "r1".into(),
            base_branch: Some("main".into()),
            ..Default::default()
        },
    )
    .unwrap();
    schema::run_idea(conn, &run.run_id, &horch_core::clock::now_stamp(), true)
        .unwrap()
        .expect("the idea of r1");
    let mut new = NewTask::work("t0", "tickets/t0.md", "opus");
    new.ticket_snapshot = Some(ticket.to_string());
    tasks::add(conn, &run, &new, Actor::Orchestrator).unwrap();
}

#[test]
fn a_task_attempt_of_a_run_with_no_orchestrator_pane_reaches_live() {
    let w = World::fleet("p6-push", true, seed_task);
    let went_live = "SELECT a.id FROM attempts a WHERE a.task_id IS NOT NULL AND a.via = 'push' \
         AND (a.state = 'live' OR EXISTS (SELECT 1 FROM events e WHERE e.entity = 'attempt' \
         AND e.entity_id = a.id AND e.text LIKE '%live%'))";
    let live = wait_for_or(
        "the push attempt to reach live",
        || w.rows(went_live).into_iter().next(),
        || w.diagnostics(),
    );
    let fleetd = std::fs::read_to_string(w.h.root.join("fleetd.stderr")).unwrap_or_default();
    assert!(!fleetd.contains("has no pane"), "{live}: {fleetd}");
}
