use std::path::Path;

use horch_core::taskledger::map::ticket_facts;
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Run, Thresholds};
use horch_core::taskledger::tasks::{self, NewTask, Task};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util, Store};

use crate::harness::Harness;

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
