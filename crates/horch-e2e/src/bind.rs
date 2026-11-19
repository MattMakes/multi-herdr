//! The bind of a ledger task in an end-to-end test (FR-5).
//!
//! No worker finds its task with `horch ledger claim`: fleetd binds each
//! task. A test that needs a task held by a worker calls [`bind`]. It binds
//! the task with `horch_core::taskledger::testing::bind_live`, the helper of
//! the fixtures of `horch-core`, in the ledger of the harness.

use horch_core::fleetd::task_execution::application::dispatch::Bound;
use horch_core::fleetd::task_execution::domain::task::Recorded;
use horch_core::taskledger::tasks::{self, Task};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{testing, Store};

use crate::harness::Harness;

/// Bind the first startable task of persona `persona` in the ledger of `h`
/// to a new worker of that persona. The attempt is `live`, the task is
/// `running` and the worker is `working`. `agent` is the PID of the agent
/// process of the worker, if the test has one.
///
/// A test uses [`Bound::worker_id`] as `HORCH_WORKER_ID` of its worker
/// commands. It panics when no task of `persona` can start.
pub fn bind(h: &Harness, persona: &str, agent: Option<u32>) -> Bound {
    try_bind(h, persona, agent).unwrap_or_else(|e| panic!("{e}"))
}

/// [`bind`] with the fields of `worker`, such as its pane. The bind takes
/// the persona of `worker`, and the run of the task it finds.
pub fn bind_worker(h: &Harness, worker: NewWorker, agent: Option<u32>) -> Bound {
    try_bind_worker(h, worker, agent).unwrap_or_else(|e| panic!("{e}"))
}

/// [`bind`] that gives the reason as an error when no task of `persona`
/// can start or the bind refuses. A test of a task that must not start
/// uses it.
pub fn try_bind(h: &Harness, persona: &str, agent: Option<u32>) -> Result<Bound, String> {
    let worker = NewWorker {
        persona: persona.to_string(),
        ..Default::default()
    };
    try_bind_worker(h, worker, agent)
}

fn try_bind_worker(h: &Harness, worker: NewWorker, agent: Option<u32>) -> Result<Bound, String> {
    let store = Store::open(&h.ledger_db()).expect("the ledger of the harness");
    let persona = worker.persona.clone();
    let Some(task) = next_task(&store, &persona) else {
        let db = h.ledger_db();
        return Err(format!(
            "no startable task of persona {persona} in {}",
            db.display()
        ));
    };
    let worker = NewWorker {
        run_id: task.run_id.clone(),
        ..worker
    };
    let agent = agent.map(|pid| Recorded {
        pid,
        pid_start: horch_core::procid::start_time(pid),
    });
    testing::bind_live(store.conn(), &task.task_id, &worker, agent)
        .map_err(|e| format!("bind {} to {persona}: {e:#}", task.short()))
}

/// The first startable task of `persona`: the runs in the order they were
/// made, and the tasks of a run in the order of the board.
fn next_task(store: &Store, persona: &str) -> Option<Task> {
    let conn = store.conn();
    let runs: Vec<String> = conn
        .prepare("SELECT run_id FROM runs ORDER BY rowid")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .expect("the runs of the ledger");
    runs.into_iter().find_map(|run_id| {
        tasks::startable(conn, &run_id)
            .expect("the startable tasks")
            .into_iter()
            .find(|t| t.claimable_by.as_deref() == Some(persona))
    })
}
