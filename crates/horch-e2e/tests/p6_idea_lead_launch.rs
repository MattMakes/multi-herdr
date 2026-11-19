//! The e2e tests of the launch of an idea-lead sub-orchestrator (T-890;
//! `docs/specs/fleetd/design.md`: FR-2, FR-7, I7).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder, and runs a test `horch fleetd` with the fake herdr and the fake
//! agents. `horch idea add` captures 1 idea. The dispatcher gives it a
//! feature, and fleetd starts 1 idea-lead attempt through the launch path
//! of each other attempt: `horch spawn idea-lead --attempt <id>`.
//!
//! A test selects the fake idea-lead as it selects the fake agent of a
//! worker: the persona `idea-lead` has the agent `claude`, and the `claude`
//! on the `PATH` of the harness is `fake-claude`. `HORCH_FAKE_SCENARIO`
//! names its keys. Each test waits for an event, never for a time.
//!
//! The pane rule is the same for each launch of fleetd: in Phase 6 no run
//! has an orchestrator pane, so a launch with no recorded pane splits the
//! first pane of the herdr workspace whose label is the project folder
//! name. [`a_task_attempt_of_a_run_with_no_orchestrator_pane_reaches_live`]
//! proves it for a push attempt of a task.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, Store};
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use serde_json::Value;

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The text of the idea of each test.
const IDEA_TEXT: &str = "Show the queue length in the status bar";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A fleet with a test fleetd and 1 idea that `horch idea add` captured.
/// With `workspace`, herdr has a workspace with the label of the project
/// folder, as `horch fleet` makes it.
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

    /// The fleet of [`World::new`] with no idea: `seed` writes rows before
    /// fleetd starts.
    fn fleet(name: &str, workspace: bool, seed: impl FnOnce(&Harness)) -> World {
        let mut h = Harness::new(name).with_git();
        // The ledger and the fleet root are outside the state folder. The
        // launch gives the pane worker FLEET_ROOT, so it finds the operator
        // token and the channel of this fleetd.
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
        // `exec`: each pane runs its command. `stay`: the fake agent stays.
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

    /// The name of the project folder: the label that `horch fleet` gives
    /// its herdr workspace.
    fn project_label(&self) -> String {
        self.h
            .project
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    /// Rows of 1 text column of the ledger.
    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = rusqlite::Connection::open(self.h.ledger_db()).unwrap();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    /// What a failed wait shows: the argv of each fake call, the output of
    /// fleetd, the log of its launches and the rows of the attempts.
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

/// The argv of the call of the fake `claude` whose argv holds `needle`.
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

/// fleetd starts the fake idea-lead of the new idea in the herdr workspace
/// of the project. Its launch argument names the idea, its phase and its
/// idea folder, and the attempt has no task.
#[test]
fn fleetd_starts_the_fake_idea_lead_of_a_new_idea() {
    let w = World::new("p6-lead", true);

    // The pass can add other ideas (a profile drift), so the test finds
    // its idea by the text of its idea.md.
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
    // The idea-lead persona ran: its profile denies the approve command.
    assert!(
        argv.iter().any(|a| a.contains("Bash(horch idea approve*)")),
        "{argv:?}"
    );
    // The first attempt of the idea is its idea-lead attempt. The launched
    // `horch spawn` exits after the pane opens, so a later pass can end the
    // attempt and start the next one (T-892 owns the process of a launch).
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

/// With no recorded pane and no herdr workspace with the label of the
/// project folder, the launch fails with a clear text and starts no agent.
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

/// The project declares its gates and commits the file, so no hold of a
/// missing fast gate stops the dispatch (Phase 4, Q-C).
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

/// The run `r1` with its idea in flight (FR-12) and 1 queued task of the
/// persona `opus`. The run has no orchestrator and no pane, as each run of
/// Phase 6.
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

/// A push attempt of a task of a run with no orchestrator pane launches in
/// the herdr workspace of the project, and reaches `live`. The ledger and
/// the fleet root are outside the state folder, so the pane worker finds
/// the operator token only through the FLEET_ROOT of the launch.
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
