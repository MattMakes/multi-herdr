//! The bind of a ledger task in a CLI test (FR-5). No worker finds its task
//! with `horch ledger claim`: fleetd binds each task. A test binds it with
//! `horch_core::taskledger::testing::bind_live`, the helper of the fixtures
//! of `horch-core`.

use std::path::Path;

use horch_core::fleetd::task_execution::application::dispatch::Bound;
use horch_core::fleetd::task_execution::domain::task::Recorded;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{tasks, testing, Store};

/// Bind the first startable task of the persona of `worker` in the ledger
/// `db` to a new worker with the fields of `worker` and the agent process
/// `agent`, if the test has one. The
/// attempt is `live`, the task is `running` and the worker is `working`.
/// The runs are read in the order they were made.
// Not each test file that uses `support` binds a task.
#[allow(dead_code)]
pub fn bind(db: &Path, worker: NewWorker, agent: Option<u32>) -> Bound {
    let store = Store::open(db).expect("the ledger of the test");
    let conn = store.conn();
    let runs: Vec<String> = conn
        .prepare("SELECT run_id FROM runs ORDER BY rowid")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .expect("the runs of the ledger");
    let persona = worker.persona.clone();
    let task = runs
        .into_iter()
        .find_map(|run_id| {
            tasks::startable(conn, &run_id)
                .expect("the startable tasks")
                .into_iter()
                .find(|t| t.claimable_by.as_deref() == Some(persona.as_str()))
        })
        .unwrap_or_else(|| panic!("no startable task of persona {persona}"));
    let worker = NewWorker {
        run_id: task.run_id.clone(),
        ..worker
    };
    let agent = agent.map(|pid| Recorded {
        pid,
        pid_start: horch_core::procid::start_time(pid),
    });
    testing::bind_live(conn, &task.task_id, &worker, agent)
        .unwrap_or_else(|e| panic!("bind {} to {persona}: {e:#}", task.short()))
}
