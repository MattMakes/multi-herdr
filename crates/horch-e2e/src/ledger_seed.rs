//! The run fixture of an end-to-end test, since Phase 6 retired
//! `horch ledger run new` and `horch ledger add` (fleetd-p6 ticket 47).
//!
//! [`seed_run`] writes a run of the project of a [`Harness`] and its idea
//! in flight (FR-12) into the ledger of the harness, and makes the run
//! branch. [`seed_task`] adds 1 queued task from a ticket file. Each write
//! is a fixture of the test, in 1 immediate transaction, so call them
//! before the work that the test checks.
//!
//! ```ignore
//! let run = seed_run(&h, "race", Some(1), "w1:p1");
//! seed_task(&h, &run, &h.project.join("race-1.md"), "codex-terra");
//! ```

use std::path::Path;

use horch_core::taskledger::map::ticket_facts;
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Run, Thresholds};
use horch_core::taskledger::tasks::{self, NewTask, Task};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util, Store};

use crate::harness::Harness;

/// The run `slug` of the project of `h`, as `horch ledger run new <slug>
/// [--heartbeat-every <s>]` made it in the pane `orchestrator_pane`:
/// active, in the workspace `HORCH_WORKSPACE_ID` of `h`, on the run branch
/// `fleet/<slug>` from `main`, with its idea in flight and its orchestrator
/// row `working` in that pane. fleetd opens the pane of each attempt of
/// the run beside it. Needs [`Harness::with_git`].
pub fn seed_run(
    h: &Harness,
    slug: &str,
    heartbeat_every_s: Option<i64>,
    orchestrator_pane: &str,
) -> Run {
    let branch = format!("fleet/{slug}");
    let out = h
        .git_cmd(&["branch", &branch, "main"])
        .output()
        .expect("running git");
    assert!(
        out.status.success(),
        "git branch {branch} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let root = h.project.canonicalize().expect("the project folder exists");
    let new = NewRun {
        slug: slug.to_string(),
        workspace_id: h.env.get("HORCH_WORKSPACE_ID").cloned(),
        base_branch: Some("main".into()),
        run_branch: Some(branch),
        thresholds: Thresholds {
            heartbeat_every_s,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut store = Store::open(&h.ledger_db()).expect("opening the ledger of the harness");
    store
        .immediate(|tx| {
            let run = runs::create(tx, &root, &new)?;
            schema::run_idea(tx, &run.run_id, &util::now(), true)?;
            let orchestrator = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    kind: Some(WorkerKind::Orchestrator),
                    persona: "orchestrator".into(),
                    role: Some("orchestrator".into()),
                    workspace_id: new.workspace_id.clone(),
                    pane_id: Some(orchestrator_pane.into()),
                    ..Default::default()
                },
            )?;
            tx.execute(
                "UPDATE workers SET status = 'working' WHERE worker_id = ?1",
                [&orchestrator],
            )?;
            runs::set_orchestrator(tx, &run.run_id, &orchestrator)?;
            runs::get(tx, &run.run_id)
        })
        .expect("seeding the run")
}

/// 1 queued task of `run` from the ticket file `ticket`, for the persona
/// `primary`, as `horch ledger add --file <ticket> --primary <primary>`
/// made it: its title and its check come from the ticket.
pub fn seed_task(h: &Harness, run: &Run, ticket: &Path, primary: &str) -> Task {
    let text = std::fs::read_to_string(ticket).expect("reading the ticket");
    let facts = ticket_facts(&text);
    let title = facts.title.clone().unwrap_or_else(|| "task".to_string());
    let new = NewTask {
        land_check: facts.check.clone(),
        ticket_snapshot: Some(text.clone()),
        ..NewTask::work(&title, &ticket.to_string_lossy(), primary)
    };
    let mut store = Store::open(&h.ledger_db()).expect("opening the ledger of the harness");
    store
        .immediate(|tx| tasks::add(tx, run, &new, Actor::Orchestrator))
        .expect("seeding the task")
}

/// 1 worker row of `run` for the persona `persona`, in the workspace of
/// `run`, with no attempt, as `horch ledger worker new --persona <persona>`
/// made it. Returns its worker id.
pub fn seed_worker(h: &Harness, run: &Run, persona: &str) -> String {
    let new = NewWorker {
        run_id: run.run_id.clone(),
        kind: Some(WorkerKind::Worker),
        persona: persona.into(),
        workspace_id: run.workspace_id.clone(),
        ..Default::default()
    };
    let mut store = Store::open(&h.ledger_db()).expect("opening the ledger of the harness");
    store
        .immediate(|tx| workers::insert(tx, &new))
        .expect("seeding the worker")
}
