//! `horch spawn`'s task-ledger side (#34): the worker row exists before the
//! pane opens, its id travels in the brief, and the role and pane are
//! recorded once the split is done.
//!
//! The command changes no row itself (RR-5, ticket 23 of fleetd-p5): each
//! change is 1 request to fleetd, and each read opens the ledger read-only.
//! Only the spawns of fleetd run: `horch spawn --attempt` of a bound
//! attempt (ticket 10 of fleetd-p4) and the restart on a fallback. Each
//! other spawn is refused (ticket 48 of fleetd-p6).

use std::path::Path;

use anyhow::{bail, ensure, Result};
use horch_core::execution::store::ExecutionStore;
use horch_core::fleetd::fleet_runtime::adapters::channel_client::{self, Sent};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::Outcome;
use horch_core::fleetd::fleet_runtime::application::holds;
use horch_core::fleetd::fleet_runtime::domain::health::Finding;
use horch_core::fleetd::fleet_runtime::domain::requests::{retired, Kind};
use horch_core::fleetd::task_execution::adapters::task_files::AttemptFiles;
use horch_core::fleetd::task_execution::application::dispatch::write_task_files;
use horch_core::fleetd::task_execution::application::quota::ADOPT_WORKER_ENV;
use horch_core::fleetd::task_execution::domain::task::AttemptState;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::{RunStatus, WorkerStatus};
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::workers;
use horch_core::taskledger::{self, Store};
use serde_json::{Map, Value};

use super::spawn::SpawnArgs;
use super::taskledger::{project_root, RunArg};

/// The first line of the pane of an adopted restart on a fallback (RR-11).
/// Its attempt began with a claim, so it ends by the old path. The Phase 2
/// bridge: ticket 10 of Phase 4 launches a restart through the bind and
/// `--attempt`.
const ADOPTED_CLAIM_LINE: &str = "Run horch ledger claim. Read the ticket file it prints and follow it exactly. If it prints that there is no task for you, send the line it prints, then end your turn.";

/// The environment variable that names the task a spawn binds first
/// (TRF-24, L2). Reviewer auto-spawn sets it on its `horch spawn` child.
pub const CLAIM_TARGET_ENV: &str = "HORCH_CLAIM_TARGET";

/// The first line of the pane of a push attempt (Q8 of
/// `docs/tickets/fleetd-p4/MAP.md`): 1 fixed format with the absolute path
/// of its `START.md`, because a pane has no task folder as its cwd.
pub fn start_line(start_md: &Path) -> String {
    format!("Read {} and continue.", start_md.display())
}

/// The line of `horch spawn`, retired by ticket 48 of fleetd-p6: fleetd
/// launches each attempt.
pub(crate) fn retired_line() -> String {
    retired("spawn").expect("spawn is a retired kind").line()
}

/// Whether fleetd launched this spawn: `--attempt`, or the restart on a
/// fallback that names its worker row in [`ADOPT_WORKER_ENV`].
pub(crate) fn launched_by_fleetd(args: &SpawnArgs) -> bool {
    args.attempt.is_some() || adopt_wanted().is_some()
}

/// Send 1 request of `kind` with `payload` to fleetd (ticket 15). Gives the
/// result of an applied request, or `None` when the request waits in a
/// request file because fleetd is down: the client printed that line. A
/// refused request is the error `refused: <reason>`.
pub(crate) fn request(ctx: &RuntimeContext, kind: Kind, payload: Value) -> Result<Option<Value>> {
    let Value::Object(payload) = payload else {
        bail!("the payload of {} is not an object", kind.as_str());
    };
    match channel_client::send(ctx, kind, payload)? {
        Sent::Replied(reply) => match reply.outcome {
            Outcome::Applied(result) => Ok(Some(result)),
            Outcome::Refused(reason) => bail!("refused: {reason}"),
        },
        Sent::Queued { .. } => Ok(None),
    }
}

/// The text field `key` of the result of a request.
fn text_of(result: &Value, key: &str) -> Result<String> {
    result[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("the reply of fleetd has no text `{key}`"))
}

/// A payload object from the fields that have a value.
fn payload(fields: &[(&str, Option<&str>)]) -> Value {
    let map: Map<String, Value> = fields
        .iter()
        .filter_map(|(k, v)| v.map(|v| (k.to_string(), Value::String(v.to_string()))))
        .collect();
    Value::Object(map)
}

/// What `horch spawn` does after its ledger side.
pub enum Prepared {
    /// The spawn joins no task-ledger run.
    NoRun,
    /// The spawn opens the pane of this worker row.
    Pane(LedgerSpawn),
    /// The spawn ends after its request, and prints this line on stdout:
    /// the worker of a bind that fleetd launches, or the id of a request
    /// that waits for fleetd.
    Ended(String),
}

/// A spawn's worker row.
pub struct LedgerSpawn {
    pub run: Run,
    pub worker_id: String,
    /// The first line of the pane: [`start_line`] of a push attempt, or the
    /// claim of an adopted restart. `None` for a role, whose brief is its
    /// task.
    pub first_line: Option<String>,
    /// The push attempt of the worker, with its files.
    pub push: Option<Pushed>,
}

/// The push attempt that fleetd bound and that a spawn opens the pane of
/// (`--attempt`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushed {
    pub attempt_id: String,
    pub task_id: String,
    pub files: AttemptFiles,
}

/// The ledger side of a spawn that joins a task-ledger run: it names one
/// with `--run` (or `HORCH_RUN_ID`), it replaces a ledger worker, its
/// workspace has an active run, or a `spawn_only` controller in an active
/// run sends a role its brief. [`Prepared::NoRun`] otherwise: a controller
/// outside every run sends its brief outside every run too.
///
/// - `--attempt`: the attempt that fleetd bound; its task files are
///   written, and no row changes.
/// - `HORCH_ADOPT_WORKER`: 1 `spawn_adopt` request adopts the row that
///   fleetd wrote for a restart.
/// - else the spawn is refused with the line of the retired `spawn` kind:
///   fleetd launches each attempt (ticket 48 of fleetd-p6).
///
/// `controller` is the caller's ledger row when the caller has
/// `spawn_only` and passed its check (T-38). Only such a spawn carries task
/// text: `horch spawn` refuses every other (TRF-43).
pub fn prepare(
    ctx: &RuntimeContext,
    args: &SpawnArgs,
    controller: Option<&str>,
) -> Result<Prepared> {
    refuse_on_hold(ctx)?;
    let Some(store) = taskledger::open_existing_read_only(ctx)? else {
        if args.run.is_some() || args.attempt.is_some() {
            bail!(
                "the ledger {} does not exist",
                ctx.paths.ledger_db.display()
            );
        }
        return Ok(Prepared::NoRun);
    };
    let role_run = controller_run(&store, args, controller)?;
    let run = match &role_run {
        Some(run) => run.clone(),
        None if !args.task.trim().is_empty() => return Ok(Prepared::NoRun),
        None => {
            let Some(explicit) = ledger_run_wanted(ctx, &store, args)? else {
                return Ok(Prepared::NoRun);
            };
            super::taskledger::resolve_run(ctx, &store, &RunArg { run: explicit })?
        }
    };
    ensure_active(&run)?;
    let persona = spawn_persona(ctx, args)?;
    ensure_replaceable(&store, args, &run)?;
    let (id, first_line, push) = match (&args.attempt, adopt_wanted()) {
        (Some(attempt), _) => {
            let (worker, pushed) = push(ctx, &store, attempt, &run, &persona)?;
            (
                worker,
                Some(start_line(&pushed.files.start_md)),
                Some(pushed),
            )
        }
        (None, Some(row)) if role_run.is_some() => {
            bail!("horch spawn refused: a controller's brief adopts no worker row ({row})")
        }
        (None, Some(row)) => {
            let target = claim_target();
            let Some(id) = adopt(ctx, &row, &run, &persona, target.as_deref())? else {
                return Ok(Prepared::Ended(String::new()));
            };
            (id, Some(ADOPTED_CLAIM_LINE.to_string()), None)
        }
        (None, None) => bail!("{}", retired_line()),
    };
    Ok(Prepared::Pane(LedgerSpawn {
        run,
        worker_id: id,
        first_line,
        push,
    }))
}

/// The push attempt `attempt` that fleetd bound (`--attempt`), with its
/// `task.yaml` and `START.md` written. Gives its worker row and the
/// attempt.
fn push(
    ctx: &RuntimeContext,
    store: &Store,
    attempt: &str,
    run: &Run,
    persona: &str,
) -> Result<(String, Pushed)> {
    let (worker, attempt_id, task_id) = bound_attempt(store, attempt, run, persona)?;
    let files = write_task_files(store.conn(), &ctx.paths.fleet_root, &attempt_id, &[])?;
    Ok((
        worker,
        Pushed {
            attempt_id,
            task_id,
            files,
        },
    ))
}

/// Adopt the worker row `worker_id` that fleetd wrote for a restart on a
/// fallback (RR-11), in place of a new row, with 1 `spawn_adopt` request.
/// The variable is an input that any process can set, so fleetd checks the
/// row and records the adoption in 1 transaction: 2 spawns that name 1 row
/// cannot both adopt it. `target` is the task that [`CLAIM_TARGET_ENV`]
/// names. `None` when the request waits for fleetd.
///
/// The Phase 2 bridge: Phase 4 replaces it with the push dispatch, which
/// binds the task to a worker before the pane opens.
fn adopt(
    ctx: &RuntimeContext,
    worker_id: &str,
    run: &Run,
    persona: &str,
    target: Option<&str>,
) -> Result<Option<String>> {
    let Some(target) = target else {
        bail!(
            "horch spawn refused: it cannot adopt worker row {worker_id}: the row is not spawned for the task that {CLAIM_TARGET_ENV} names"
        );
    };
    let pid = std::process::id().to_string();
    let sent = payload(&[
        ("run_id", Some(&run.run_id)),
        ("worker_id", Some(worker_id)),
        ("persona", Some(persona)),
        ("target_task_id", Some(target)),
        ("spawn_pid", Some(&pid)),
    ]);
    match request(ctx, Kind::SpawnAdopt, sent)? {
        Some(result) => Ok(Some(text_of(&result, "worker_id")?)),
        None => Ok(None),
    }
}

/// The push attempt `attempt` that fleetd bound, for `--attempt`: it is in
/// `launching`, it began with a push dispatch, and its worker is a
/// `spawning` row of `run` and `persona` with no agent. Gives its worker,
/// the attempt and its task.
fn bound_attempt(
    store: &Store,
    attempt: &str,
    run: &Run,
    persona: &str,
) -> Result<(String, String, String)> {
    let refusal =
        |why: &str| anyhow::anyhow!("horch spawn refused: it cannot open attempt {attempt}: {why}");
    let row: Option<(Option<String>, String, String, String)> = store
        .conn()
        .query_row(
            "SELECT task_id, worker_id, state, via FROM attempts WHERE id = ?1",
            [attempt],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;
    let Some((task, worker_id, state, via)) = row else {
        return Err(refusal("no attempt row has this id"));
    };
    let Some(task) = task else {
        return Err(refusal("the attempt has no task"));
    };
    if via != "push" {
        return Err(refusal("the attempt began with a claim"));
    }
    if state != AttemptState::Launching.as_str() {
        return Err(refusal(&format!("the attempt is {state}, not launching")));
    }
    let w = workers::get(store.conn(), &worker_id)?;
    if w.run_id.as_deref() != Some(run.run_id.as_str()) {
        return Err(refusal(&format!("its worker is not of run {}", run.slug)));
    }
    if w.persona.as_deref() != Some(persona) {
        return Err(refusal(&format!("its worker is not of persona {persona}")));
    }
    if w.status != WorkerStatus::Spawning.as_str() || w.agent_pid.is_some() {
        return Err(refusal("its worker has started"));
    }
    Ok((worker_id, attempt.to_string(), task))
}

/// Refuse a spawn while a health guard holds new dispatch (OR-5), before
/// any row is written. The refusal names each hold with its floor and its
/// reading. A guard with no reading prints 1 note and does not hold
/// (`preservation-06`, row S12).
fn refuse_on_hold(ctx: &RuntimeContext) -> Result<()> {
    let mut held = Vec::new();
    for finding in holds::holds(ctx)? {
        match finding {
            Finding::Hold(h) => held.push(h.cause().to_string()),
            Finding::Unknown { fact, why } => eprintln!(
                "horch: NOTE: the health guard {} does not hold dispatch: {why}",
                fact.as_str()
            ),
        }
    }
    if held.is_empty() {
        return Ok(());
    }
    bail!(
        "horch spawn refused: new dispatch is on hold until the machine recovers:\n  - {}",
        held.join("\n  - ")
    )
}

/// Whether this spawn joins a task-ledger run. `None`: it does not. `Some`
/// holds the run it joins: the run `--run` (or `HORCH_RUN_ID`) names, the run
/// of the worker `--replaces` names, else the 1 active run of the caller's
/// herdr workspace (TRF-34). A planned run there wins over solo runs. A spawn never joins the run of another
/// workspace unless it names that run.
fn ledger_run_wanted(
    ctx: &RuntimeContext,
    store: &Store,
    args: &SpawnArgs,
) -> Result<Option<Option<String>>> {
    let workspace = super::sessionscmd::caller_workspace(ctx);
    run_to_join(
        store,
        &project_root(ctx)?,
        explicit_run(args),
        workspace.as_deref(),
        args,
    )
}

/// [`ledger_run_wanted`] with its inputs read.
fn run_to_join(
    store: &Store,
    root: &Path,
    explicit: Option<String>,
    workspace: Option<&str>,
    args: &SpawnArgs,
) -> Result<Option<Option<String>>> {
    if explicit.is_some() {
        return Ok(Some(explicit));
    }
    if let Some(prev) = &args.replaces {
        if let Some(run) = workers::find(store.conn(), prev)?.and_then(|w| w.run_id) {
            return Ok(Some(Some(run)));
        }
    }
    let active = active_runs(store, root)?;
    // A solo run never makes the workspace's planned run ambiguous (TRF-34).
    let own = runs::planned_first(
        active
            .iter()
            .filter(|r| workspace.is_some() && r.workspace_id.as_deref() == workspace)
            .collect(),
    );
    match own.as_slice() {
        [one] => Ok(Some(Some(one.run_id.clone()))),
        [] => {
            ensure!(
                args.replaces.is_none(),
                "--replaces needs an active task-ledger run in {}",
                root.display()
            );
            note_other_workspaces(&active, workspace);
            Ok(None)
        }
        many => bail!(
            "{} active runs in workspace {}: {}. Name one with --run <slug>",
            many.len(),
            workspace.unwrap_or_default(),
            many.iter()
                .map(|r| r.slug.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Say that a spawn joins no run, because each active run of the
/// project belongs to another workspace than the caller's.
fn note_other_workspaces(active: &[Run], workspace: Option<&str>) {
    if active.is_empty() {
        return;
    }
    let runs: Vec<String> = active
        .iter()
        .map(|r| {
            format!(
                "{} (workspace {})",
                r.slug,
                r.workspace_id.as_deref().unwrap_or("(none)")
            )
        })
        .collect();
    eprintln!(
        "horch spawn: NOTE: this spawn joins no task-ledger run. Run {} is not of this workspace {}. Give --run <slug> to join it.",
        runs.join(", "),
        workspace.unwrap_or("(none)")
    );
}

/// The run a role joins (T-38, decision Q4): the active run of the
/// `spawn_only` controller's ledger row, when the spawn has task text and
/// names no run. Only a controller passes `controller`, so every other
/// spawn with task text keeps #89.
fn controller_run(
    store: &Store,
    args: &SpawnArgs,
    controller: Option<&str>,
) -> Result<Option<Run>> {
    let Some(wid) = controller else {
        return Ok(None);
    };
    if explicit_run(args).is_some() || args.task.trim().is_empty() {
        return Ok(None);
    }
    let Some(run_id) = workers::find(store.conn(), wid)?.and_then(|w| w.run_id) else {
        return Ok(None);
    };
    let run = runs::get(store.conn(), &run_id)?;
    Ok((run.status == RunStatus::Active.as_str()).then_some(run))
}

/// The run `--run` or `HORCH_RUN_ID` names.
fn explicit_run(args: &SpawnArgs) -> Option<String> {
    args.run
        .clone()
        .or_else(|| std::env::var("HORCH_RUN_ID").ok().filter(|v| !v.is_empty()))
}

/// The active runs of the project.
fn active_runs(store: &Store, root: &Path) -> Result<Vec<Run>> {
    let all = runs::of_project(store.conn(), root)?;
    Ok(all
        .into_iter()
        .filter(|r| r.status == RunStatus::Active.as_str())
        .collect())
}

/// The worker row that [`ADOPT_WORKER_ENV`] names, if it is set: fleetd
/// wrote it for a restart on a fallback.
fn adopt_wanted() -> Option<String> {
    std::env::var(ADOPT_WORKER_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// The task [`CLAIM_TARGET_ENV`] names, if it is set.
fn claim_target() -> Option<String> {
    std::env::var(CLAIM_TARGET_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// A closed run takes no new worker.
fn ensure_active(run: &Run) -> Result<()> {
    ensure!(
        run.status == RunStatus::Active.as_str(),
        "run {} is {}; a closed run takes no new worker",
        run.slug,
        run.status
    );
    Ok(())
}

/// The persona of the new worker: the teammate, or the tier of the
/// execution `--resume` names.
fn spawn_persona(ctx: &RuntimeContext, args: &SpawnArgs) -> Result<String> {
    match (&args.teammate, &args.resume) {
        (Some(t), _) => Ok(t.clone()),
        (None, Some(key)) => Ok(ExecutionStore::open_in(ctx)?.get(key)?.tier),
        (None, None) => bail!("give a teammate or --resume <id>"),
    }
}

/// Refuse to replace a worker that is still live.
fn ensure_replaceable(store: &Store, args: &SpawnArgs, run: &Run) -> Result<()> {
    let Some(prev) = &args.replaces else {
        return Ok(());
    };
    let p = workers::get(store.conn(), prev)?;
    ensure!(
        !p.status()?.is_live(),
        "worker {} is still {}. Run horch ledger stop {} \"<reason>\" --run {}, then spawn again",
        p.label(),
        p.status,
        p.worker_id,
        run.slug
    );
    Ok(())
}

impl LedgerSpawn {
    /// The pane is open and runs `horch worker <role>`: 1 `worker_launched`
    /// request records it. A failed record prints a warning and never fails
    /// the spawn.
    pub fn spawned(
        &self,
        ctx: &RuntimeContext,
        role: &str,
        pane: &str,
        harness: &str,
        model: &str,
        effort: Option<&str>,
    ) {
        let sent = payload(&[
            ("run_id", Some(&self.run.run_id)),
            ("worker_id", Some(&self.worker_id)),
            ("pane", Some(pane)),
            ("role", Some(role)),
            ("harness", Some(harness)),
            ("model", Some(model)),
            ("effort", effort),
        ]);
        if let Err(e) = request(ctx, Kind::WorkerLaunched, sent) {
            eprintln!("horch spawn: the task ledger did not record pane {pane}: {e:#}");
        }
        eprintln!(
            "horch spawn: task ledger worker {} (run {}).",
            self.worker_id, self.run.slug
        );
    }

    /// The spawn failed before the worker ran: 1 `worker_launch_failed`
    /// request ends its row.
    pub fn launch_failed(&self, ctx: &RuntimeContext, reason: &str) {
        let sent = payload(&[
            ("run_id", Some(&self.run.run_id)),
            ("worker_id", Some(&self.worker_id)),
            ("reason", Some(reason)),
        ]);
        if let Err(e) = request(ctx, Kind::WorkerLaunchFailed, sent) {
            eprintln!("horch spawn: the task ledger did not record the failed start: {e:#}");
        }
    }
}

/// An in-process fleetd for the tests of `horch spawn`: the channel server
/// of ticket 13 with the worker handlers of ticket 19, on the ledger and
/// the fleet root of a test context. The `fleetd` row names this process,
/// so the client of the channel accepts the server.
#[cfg(test)]
pub(crate) mod test_fleetd {
    use std::time::Duration;

    use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
    use horch_core::fleetd::fleet_runtime::adapters::token_file;
    use horch_core::fleetd::fleet_runtime::application::handlers_worker::{register, WorkerDeps};
    use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
    use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
    use horch_core::runtime::RuntimeContext;
    use horch_core::taskledger::util;

    /// Serve the channel of `ctx` until the server is dropped.
    pub fn serve(ctx: &RuntimeContext) -> ChannelServer {
        let conn = rusqlite::Connection::open(&ctx.paths.ledger_db).unwrap();
        conn.busy_timeout(Duration::from_secs(10)).unwrap();
        let pid = std::process::id();
        let start = horch_platform::process::start_time(pid).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, ?1, ?2, ?3, 0, ?3)",
            rusqlite::params![pid, start as i64, util::now()],
        )
        .unwrap();
        let root = &ctx.paths.fleet_root;
        std::fs::create_dir_all(root).unwrap();
        token_file::ensure(root).unwrap();
        let deps = WorkerDeps {
            guard: Generation(1),
        };
        let mut handlers = Handlers::new();
        register(&mut handlers, &deps);
        ChannelServer::start(root, conn, Generation(1), handlers).unwrap()
    }
}

#[cfg(test)]
mod trf_33_34_tests {
    use std::time::Duration;

    use horch_core::execution::TilingMode;
    use horch_core::taskledger::model::{Actor, StopRequester, WorkerKind};
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::stop::{self, Panes, Processes};
    use horch_core::taskledger::tasks::{self, NewTask};
    use horch_core::taskledger::workers::NewWorker;
    use horch_core::workspace::model::Direction;

    use super::*;

    fn args(teammate: Option<&str>) -> SpawnArgs {
        SpawnArgs {
            teammate: teammate.map(str::to_string),
            task: String::new(),
            resume: None,
            phase: None,
            effort: None,
            model: None,
            role: None,
            from_pane: None,
            direction: Direction::Right,
            tiling: TilingMode::Disabled,
            exact: false,
            force: false,
            replaces: None,
            run: None,
            attempt: None,
        }
    }

    /// A ledger with 1 active run per (slug, workspace) in project `root`.
    fn world(runs_of: &[(&str, Option<&str>)]) -> (tempfile::TempDir, Store, Vec<Run>) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let made = runs_of
            .iter()
            .map(|(slug, ws)| {
                store
                    .immediate(|tx| {
                        let run = runs::create(
                            tx,
                            dir.path(),
                            &NewRun {
                                slug: (*slug).into(),
                                workspace_id: ws.map(str::to_string),
                                ..Default::default()
                            },
                        )?;
                        // FR-12: a task needs an idea in flight of the feature of its run.
                        horch_core::taskledger::schema::run_idea(
                            tx,
                            &run.run_id,
                            &horch_core::taskledger::util::now(),
                            true,
                        )?;
                        Ok(run)
                    })
                    .unwrap()
            })
            .collect();
        (dir, store, made)
    }

    /// A machine fixture with `free_gb` GB of free disk; each other
    /// reading is healthy.
    fn machine_file(dir: &Path, free_gb: u64) -> std::path::PathBuf {
        let path = dir.join(format!("machine-{free_gb}.json"));
        let gb: u64 = 1 << 30;
        let snapshot = serde_json::json!({
            "os": "macos",
            "arch": "arm64",
            "cpus": 8,
            "load_average_1m": 1.0,
            "mem_total_bytes": 16 * gb,
            "mem_available_bytes": 8 * gb,
            "disk_free_bytes": free_gb * gb,
            "disk_total_bytes": 500 * gb,
            "gpu": "none",
            "max_open_files": 256,
            "max_processes": 100_000,
        });
        std::fs::write(&path, snapshot.to_string()).unwrap();
        path
    }

    #[test]
    fn ledger_spawn_refuses_with_the_hold_and_its_cause() {
        let (dir, store, made) = world(&[("r1", None)]);
        let ctx_with = |free_gb: u64| {
            let machine = machine_file(dir.path(), free_gb);
            RuntimeContext::from_env(
                &horch_core::runtime::MapEnv::new("/")
                    .with(
                        "HORCH_STATE_DIR",
                        dir.path().join("state").to_str().unwrap(),
                    )
                    .with(
                        "HORCH_LEDGER_DB",
                        dir.path().join("ledger.sqlite3").to_str().unwrap(),
                    )
                    .with("FLEET_ROOT", dir.path().join("fleet").to_str().unwrap())
                    .with("HORCH_PROJECT_DIR", dir.path().to_str().unwrap())
                    .with("HORCH_MACHINE_FILE", machine.to_str().unwrap()),
            )
            .unwrap()
        };
        let mut spawn = args(Some("opus"));
        spawn.run = Some(made[0].slug.clone());
        let workers = || -> i64 {
            store
                .conn()
                .query_row("SELECT COUNT(*) FROM workers", [], |r| r.get(0))
                .unwrap()
        };

        let err = prepare(&ctx_with(2), &spawn, None)
            .err()
            .expect("a refusal");
        assert_eq!(
            err.to_string(),
            "horch spawn refused: new dispatch is on hold until the machine recovers:\n  \
             - free disk 2.0 GB is below the floor of 10 GB (health_min_free_disk_gb)"
        );
        assert_eq!(workers(), 0, "a refused spawn writes no row");

        // A normal probe passes the guard. A spawn that fleetd did not
        // launch is then refused with the line of the retired kind.
        let err = prepare(&ctx_with(200), &spawn, None)
            .err()
            .expect("a retired spawn");
        assert_eq!(err.to_string(), retired_line());
        assert_eq!(workers(), 0);
    }

    /// A resume spawn joins the run of its workspace, so it binds a task
    /// as each worker spawn does (TRF-33).
    #[test]
    fn trf_33_a_resume_spawn_joins_the_run_of_its_workspace() {
        let (dir, store, made) = world(&[("r1", Some("w1"))]);
        let mut resume = args(None);
        resume.resume = Some("s-1".into());
        let joined = run_to_join(&store, dir.path(), None, Some("w1"), &resume).unwrap();
        assert_eq!(joined, Some(Some(made[0].run_id.clone())));
        // A spawn outside every run gets no task.
        assert_eq!(super::super::spawn::worker_task(None), "");
    }

    #[test]
    fn trf_34_a_task_less_spawn_joins_only_its_own_workspaces_run() {
        let (dir, store, made) = world(&[("r1", Some("w1")), ("r2", Some("w2"))]);
        let root = dir.path();
        let bare = args(Some("opus"));
        let join = |ws: Option<&str>, explicit: Option<&str>| {
            run_to_join(&store, root, explicit.map(str::to_string), ws, &bare).unwrap()
        };
        assert_eq!(join(Some("w1"), None), Some(Some(made[0].run_id.clone())));
        assert_eq!(join(Some("w2"), None), Some(Some(made[1].run_id.clone())));
        // Another workspace's 1 active run is not joined.
        assert_eq!(join(Some("w3"), None), None);
        assert_eq!(join(None, None), None);
        // --run reaches another workspace's run.
        assert_eq!(join(Some("w1"), Some("r2")), Some(Some("r2".to_string())));

        // 1 run of another workspace only: still not joined.
        let (dir, store, _) = world(&[("r9", Some("w9"))]);
        assert_eq!(
            run_to_join(&store, dir.path(), None, Some("w1"), &bare).unwrap(),
            None
        );
    }

    /// TRF-34: a solo run (`horch ledger task`) beside the workspace's
    /// planned run does not make a plain spawn ambiguous.
    #[test]
    fn trf_34_a_plain_spawn_joins_the_planned_run_beside_a_solo_run() {
        let (dir, store, made) = world(&[("solo-w1", Some("w1")), ("plan", Some("w1"))]);
        let bare = args(Some("opus"));
        let join = |explicit: Option<&str>| {
            run_to_join(
                &store,
                dir.path(),
                explicit.map(str::to_string),
                Some("w1"),
                &bare,
            )
            .unwrap()
        };
        assert_eq!(join(None), Some(Some(made[1].run_id.clone())));
        // --run still reaches the solo run.
        assert_eq!(join(Some("solo-w1")), Some(Some("solo-w1".to_string())));

        // A solo run alone is still joined.
        let (dir, store, made) = world(&[("solo-w1", Some("w1"))]);
        assert_eq!(
            run_to_join(&store, dir.path(), None, Some("w1"), &bare).unwrap(),
            Some(Some(made[0].run_id.clone()))
        );
    }

    #[test]
    fn trf_34_two_planned_runs_still_refuse() {
        let (dir, store, _) = world(&[
            ("solo-w1", Some("w1")),
            ("p1", Some("w1")),
            ("p2", Some("w1")),
        ]);
        let err = run_to_join(&store, dir.path(), None, Some("w1"), &args(Some("opus")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("2 active runs in workspace w1: "), "{err}");
        assert!(err.contains("p1") && err.contains("p2"), "{err}");
        assert!(!err.contains("solo-w1"), "{err}");
    }

    struct NoPanes;
    impl Panes for NoPanes {
        fn exists(&self, _: &str) -> bool {
            false
        }
        fn close(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }
    struct NoProcs;
    impl Processes for NoProcs {
        fn is_same(&self, _: u32, _: u64) -> bool {
            false
        }
        fn exists(&self, _: u32) -> bool {
            false
        }
        fn stop_same(&self, _: u32, _: u64, _: horch_core::procid::Stop) -> bool {
            false
        }
    }

    /// Ticket 19's `refuse_over_plan` guard: a `--replaces` spawn after a
    /// reap that requeued the task is not refused.
    #[test]
    fn trf_33_a_replaces_spawn_after_a_reap_and_requeue_is_not_refused() {
        let (dir, mut store, made) = world(&[("r1", Some("w1"))]);
        let run = made[0].clone();
        store
            .immediate(|tx| {
                // No git repository: the task works in the shared checkout.
                let mut alpha = NewTask::work("alpha", "tickets/a.md", "opus");
                alpha.worktree_mode = horch_core::taskledger::model::WorktreeMode::Shared;
                tasks::add(tx, &run, &alpha, Actor::Orchestrator)
            })
            .unwrap();
        let new_row = |store: &mut Store, prev: Option<String>| {
            store
                .immediate(|tx| {
                    workers::insert(
                        tx,
                        &NewWorker {
                            run_id: run.run_id.clone(),
                            kind: Some(WorkerKind::Worker),
                            persona: "opus".into(),
                            workspace_id: Some("w1".into()),
                            previous_worker_id: prev.clone(),
                            ..Default::default()
                        },
                    )
                })
                .unwrap()
        };
        // fleetd binds the task to the first worker (FR-5).
        let task = tasks::of_run(store.conn(), &run.run_id).unwrap()[0]
            .task_id
            .clone();
        let new = NewWorker {
            run_id: run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "opus".into(),
            workspace_id: Some("w1".into()),
            ..Default::default()
        };
        let first = horch_core::taskledger::testing::bind_live(store.conn(), &task, &new, None)
            .unwrap()
            .worker_id;
        stop::reap(
            &mut store,
            &first,
            StopRequester::Orchestrator,
            "pane gone",
            &NoPanes,
            &NoProcs,
            Duration::ZERO,
        )
        .unwrap();
        let t = &tasks::of_run(store.conn(), &run.run_id).unwrap()[0];
        assert_eq!(t.status, "ready", "the reap requeues the task");

        let mut replace = args(Some("opus"));
        replace.replaces = Some(first.clone());
        let joined = run_to_join(&store, dir.path(), None, Some("w1"), &replace).unwrap();
        assert_eq!(joined, Some(Some(run.run_id.clone())));
        ensure_replaceable(&store, &replace, &run).unwrap();
        let second = new_row(&mut store, Some(first));
        assert_eq!(
            tasks::spawn_refusal(store.conn(), &run.run_id, &second).unwrap(),
            None
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use horch_core::taskledger::model::{Actor, EventKind, WorkerKind};
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::util::{self, About};
    use horch_core::taskledger::workers::NewWorker;

    use super::*;

    /// T-38 step 3: the watchdog's pre-claim sweep reports a `claiming`
    /// worker past the claim grace, but never a role that `prepare` marked.
    /// The role is not ended either: its supervisor checks in.
    #[test]
    fn a_marked_role_is_never_reported_as_a_worker_that_did_not_claim() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (run, role, plain) = s
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/r"),
                    &NewRun {
                        slug: "s".into(),
                        ..Default::default()
                    },
                )?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                let new = |persona: &str| NewWorker {
                    run_id: run.run_id.clone(),
                    kind: Some(WorkerKind::Worker),
                    persona: persona.into(),
                    ..Default::default()
                };
                let role = workers::insert(tx, &new("intake-analyst"))?;
                let plain = workers::insert(tx, &new("opus"))?;
                Ok((run, role, plain))
            })
            .unwrap();
        // The verdict of the `spawn` handler for a role (ticket 19).
        s.immediate(|tx| {
            util::event(
                tx,
                &About {
                    run: Some(&run.run_id),
                    task: None,
                    worker: Some(&role),
                },
                Actor::Horch,
                EventKind::Verdict,
                "No claim: a role of controller worker w-controller; it works from its brief and claims no task.",
            )
        })
        .unwrap();
        s.conn()
            .execute(
                "UPDATE workers SET status = 'claiming', started_at = '2000-01-01T00:00:00Z',
                    last_checkin_at = ?1",
                [util::now()],
            )
            .unwrap();
        let swept = s
            .immediate(|tx| workers::sweep_preclaim(tx, &run, &|_| true))
            .unwrap();
        let noted: Vec<&str> = swept.iter().map(|p| p.worker.worker_id.as_str()).collect();
        assert_eq!(noted, [plain.as_str()], "{swept:?}");
        assert!(swept.iter().all(|p| p.ended.is_none()), "{swept:?}");
    }
}

#[cfg(test)]
mod adoption_tests {
    use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
    use horch_core::fleetd::task_execution::adapters::sqlite as attempts_store;
    use horch_core::fleetd::task_execution::domain::task::AttemptRow;
    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::model::{Actor, WorkerKind};
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::tasks::{self, NewTask};
    use horch_core::taskledger::util;
    use horch_core::taskledger::workers::NewWorker;

    use super::*;

    /// The server ends before its folder goes: fields drop in order.
    struct World {
        _fleetd: Option<ChannelServer>,
        dir: tempfile::TempDir,
        store: Store,
        run: Run,
        task: String,
    }

    /// A run with 1 task of `dev` and a second run.
    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let (run, task) = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    dir.path(),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                let t = tasks::add(
                    tx,
                    &run,
                    &NewTask::work("a", "tickets/a.md", "dev"),
                    Actor::Orchestrator,
                )?;
                Ok((run, t.task_id))
            })
            .unwrap();
        let mut w = World {
            dir,
            store,
            run,
            task,
            _fleetd: None,
        };
        w._fleetd = Some(super::test_fleetd::serve(&w.ctx()));
        w
    }

    impl World {
        fn ctx(&self) -> RuntimeContext {
            let p = |name: &str| self.dir.path().join(name).to_str().unwrap().to_string();
            let env = MapEnv::new("/")
                .with("HORCH_STATE_DIR", &p("state"))
                .with("HORCH_DATA_DIR", &p("data"))
                .with("HORCH_LEDGER_DB", &p("ledger.sqlite3"))
                .with("FLEET_ROOT", &p("fleet"))
                .with("HORCH_PROJECT_DIR", &p(""));
            RuntimeContext::from_env(&env).unwrap()
        }

        /// The rows that fleetd writes for a restart on `alt`: a worker in
        /// `spawning` for the task, with `attempts` attempts of `state`.
        fn restart_row(&mut self, attempts: &[AttemptState]) -> String {
            let (run_id, task) = (self.run.run_id.clone(), self.task.clone());
            self.store
                .immediate(|tx| {
                    let id = workers::insert(
                        tx,
                        &NewWorker {
                            run_id: run_id.clone(),
                            kind: Some(WorkerKind::Worker),
                            persona: "alt".into(),
                            target_task_id: Some(task.clone()),
                            ..Default::default()
                        },
                    )?;
                    for (n, state) in attempts.iter().enumerate() {
                        let a = AttemptRow {
                            id: format!("{task}/A{n}-{id}"),
                            task_id: Some(task.clone()),
                            worker_id: id.clone(),
                            state: *state,
                            process: None,
                            pane: None,
                        };
                        attempts_store::insert_attempt(tx, &a, &util::now())?;
                    }
                    Ok(id)
                })
                .unwrap()
        }

        fn adopt(&mut self, row: &str, persona: &str, target: Option<&str>) -> Result<String> {
            let run = self.run.clone();
            adopt(&self.ctx(), row, &run, persona, target)
                .map(|id| id.expect("fleetd serves the request"))
        }

        fn adopted_events(&self, row: &str) -> i64 {
            self.store
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE entity_id = ?1 AND cause = ?2",
                    [row, "adopted_by_spawn"],
                    |r| r.get(0),
                )
                .unwrap()
        }
    }

    fn refused(r: Result<String>, why: &str) {
        let e = format!("{:#}", r.expect_err(why));
        assert!(e.contains("horch spawn refused: it cannot adopt"), "{e}");
        assert!(e.contains(why), "{e} does not say {why}");
    }

    #[test]
    fn spawn_adopts_the_row_that_fleetd_wrote_once() {
        let mut w = world();
        let row = w.restart_row(&[AttemptState::Launching]);
        let task = w.task.clone();
        assert_eq!(w.adopt(&row, "alt", Some(&task)).unwrap(), row);
        assert_eq!(w.adopted_events(&row), 1);
        refused(
            w.adopt(&row, "alt", Some(&task)),
            "another spawn adopted the row",
        );
        assert_eq!(w.adopted_events(&row), 1);
    }

    #[test]
    fn spawn_refuses_each_row_that_it_cannot_adopt() {
        let mut w = world();
        let task = w.task.clone();
        refused(
            w.adopt("no-such-row", "alt", Some(&task)),
            "no worker row has this id",
        );

        let row = w.restart_row(&[AttemptState::Launching]);
        let other = w
            .store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    w.dir.path(),
                    &NewRun {
                        slug: "r2".into(),
                        ..Default::default()
                    },
                )?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        refused(
            adopt(&w.ctx(), &row, &other, "alt", Some(&task)).map(|id| id.unwrap()),
            "not a row of run r2",
        );
        refused(
            w.adopt(&row, "dev", Some(&task)),
            "not a row of persona dev",
        );
        refused(w.adopt(&row, "alt", None), "not spawned for the task");
        refused(
            w.adopt(&row, "alt", Some("other-task")),
            "not spawned for the task",
        );

        let row = w.restart_row(&[]);
        refused(
            w.adopt(&row, "alt", Some(&task)),
            "exactly 1 attempt in launching",
        );
        let row = w.restart_row(&[AttemptState::Ended]);
        refused(
            w.adopt(&row, "alt", Some(&task)),
            "exactly 1 attempt in launching",
        );
        let row = w.restart_row(&[AttemptState::Launching, AttemptState::Launching]);
        refused(
            w.adopt(&row, "alt", Some(&task)),
            "exactly 1 attempt in launching",
        );

        let row = w.restart_row(&[AttemptState::Launching]);
        w.store
            .immediate(|tx| workers::agent_started(tx, &row, 4_242, Some(7)))
            .unwrap();
        refused(
            w.adopt(&row, "alt", Some(&task)),
            "the row has an agent PID",
        );

        let row = w.restart_row(&[AttemptState::Launching]);
        w.store
            .immediate(|tx| workers::launch_failed(tx, &row, "x"))
            .unwrap();
        refused(w.adopt(&row, "alt", Some(&task)), "not spawning");
        assert_eq!(w.adopted_events(&row), 0);
    }

    #[test]
    fn two_spawns_that_name_1_row_cannot_both_adopt_it() {
        let mut w = world();
        let row = w.restart_row(&[AttemptState::Launching]);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let spawns: Vec<_> = (0..2)
            .map(|_| {
                let (ctx, row, run, task) = (w.ctx(), row.clone(), w.run.clone(), w.task.clone());
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    adopt(&ctx, &row, &run, "alt", Some(&task)).is_ok()
                })
            })
            .collect();
        let adopted: Vec<bool> = spawns.into_iter().map(|s| s.join().unwrap()).collect();
        assert_eq!(adopted.iter().filter(|a| **a).count(), 1, "{adopted:?}");
        assert_eq!(w.adopted_events(&row), 1);
    }
}

#[cfg(test)]
mod push_tests {
    use horch_core::execution::TilingMode;
    use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
    use horch_core::fleetd::fleet_runtime::adapters::clock_store;
    use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
    use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
    use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
    use horch_core::fleetd::task_execution::application::dispatch::{self, DispatchCtx};
    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::git::Git;
    use horch_core::taskledger::model::{Actor, WorktreeMode};
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::tasks;
    use horch_core::taskledger::tasks::NewTask;
    use horch_core::taskledger::workers::NewWorker;
    use horch_core::workspace::model::Direction;

    use super::*;

    const HEALTHY: &str = r#"{"os":"macos","arch":"arm64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":256,"max_processes":100000}"#;

    /// The server ends before its folder goes: fields drop in order.
    struct World {
        _fleetd: Option<ChannelServer>,
        dir: tempfile::TempDir,
        store: Store,
        run: Run,
    }

    /// A ledger with the active run `r1` in temp folders, and a healthy
    /// machine fixture.
    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("machine.json"), HEALTHY).unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    dir.path(),
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        ..Default::default()
                    },
                )?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        let mut w = World {
            dir,
            store,
            run,
            _fleetd: None,
        };
        w._fleetd = Some(super::test_fleetd::serve(&w.ctx(None)));
        w
    }

    impl World {
        fn fleet_root(&self) -> std::path::PathBuf {
            self.dir.path().join("fleet")
        }

        fn ctx(&self, faults: Option<&str>) -> RuntimeContext {
            let p = |name: &str| self.dir.path().join(name).to_str().unwrap().to_string();
            let mut env = MapEnv::new("/")
                .with("HORCH_STATE_DIR", &p("state"))
                .with("HORCH_DATA_DIR", &p("data"))
                .with("HORCH_LEDGER_DB", &p("ledger.sqlite3"))
                .with("FLEET_ROOT", &p("fleet"))
                .with("HORCH_PROJECT_DIR", &p(""))
                .with("HORCH_MACHINE_FILE", &p("machine.json"));
            if let Some(f) = faults {
                env = env.with("HORCH_FAULT", f);
            }
            RuntimeContext::from_env(&env).unwrap()
        }

        /// A ready shared-checkout task of `persona`, so no git tree is
        /// needed.
        fn task(&mut self, title: &str, persona: &str) -> String {
            let mut new = NewTask::work(title, &format!("tickets/{title}.md"), persona);
            new.worktree_mode = WorktreeMode::Shared;
            new.ticket_snapshot = Some(format!("# {title}\n"));
            let run = self.run.clone();
            self.store
                .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
                .unwrap()
                .task_id
        }

        fn count(&self, sql: &str) -> i64 {
            self.store.conn().query_row(sql, [], |r| r.get(0)).unwrap()
        }
    }

    fn args(persona: &str) -> SpawnArgs {
        SpawnArgs {
            teammate: Some(persona.into()),
            task: String::new(),
            resume: None,
            phase: None,
            effort: None,
            model: None,
            role: None,
            from_pane: None,
            direction: Direction::Right,
            tiling: TilingMode::Disabled,
            exact: false,
            force: false,
            replaces: None,
            run: Some("r1".into()),
            attempt: None,
        }
    }

    /// The bind of the 1 ready task of persona `dev` to 1 push attempt in
    /// `launching`, as the dispatch of fleetd does it. Gives its worker.
    fn bind(w: &World) -> String {
        let task: String = w
            .store
            .conn()
            .query_row(
                "SELECT task_id FROM tasks WHERE status = 'ready' AND claimable_by = 'dev'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let mut conn = rusqlite::Connection::open(w.dir.path().join("ledger.sqlite3")).unwrap();
        let tx = conn.transaction().unwrap();
        let config = FleetConfig::default();
        let git = Git::new("git");
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            },
            clock: clock_store::read(&tx).unwrap(),
            git: &git,
        };
        let bound = dispatch::bind(&tx, &ctx, &task, "dev", &NewWorker::default()).unwrap();
        tx.commit().unwrap();
        bound.worker_id
    }

    /// The attempt of the worker `worker`.
    fn attempt_of(w: &World, worker: &str) -> String {
        w.store
            .conn()
            .query_row(
                "SELECT id FROM attempts WHERE worker_id = ?1",
                [worker],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// A spawn that fails before its agent runs records it with 1
    /// `worker_launch_failed` request: its worker row ends `launch_failed`.
    #[test]
    fn a_failed_spawn_records_launch_failed_through_fleetd() {
        let mut w = world();
        w.task("a", "dev");
        let worker = bind(&w);
        let mut again = args("dev");
        again.attempt = Some(attempt_of(&w, &worker));
        let ctx = w.ctx(None);
        let Prepared::Pane(l) = prepare(&ctx, &again, None).unwrap() else {
            panic!("--attempt opens a pane");
        };
        l.launch_failed(&ctx, "REFUSED: dev cannot start.");
        assert_eq!(
            w.count("SELECT COUNT(*) FROM workers WHERE status = 'launch_failed'"),
            1
        );
        assert_eq!(
            w.count(
                "SELECT COUNT(*) FROM requests WHERE kind = 'worker_launch_failed' AND state = 'applied'"
            ),
            1
        );
    }

    /// `--attempt` opens the pane of an attempt that fleetd bound: it binds
    /// nothing, sends no request, and refuses an attempt that began with a
    /// claim.
    #[test]
    fn spawn_of_a_bound_attempt_binds_nothing() {
        let mut w = world();
        let task = w.task("a", "dev");
        w.task("b", "dev");
        let attempt = attempt_of(&w, &bind(&w));

        let mut again = args("dev");
        again.attempt = Some(attempt.clone());
        let Prepared::Pane(l) = prepare(&w.ctx(None), &again, None).unwrap() else {
            panic!("--attempt opens a pane");
        };
        let push = l.push.unwrap();
        assert_eq!(
            (push.task_id.as_str(), push.attempt_id.as_str()),
            (task.as_str(), attempt.as_str())
        );
        assert_eq!(
            w.count("SELECT COUNT(*) FROM attempts"),
            1,
            "no second bind"
        );
        assert_eq!(w.count("SELECT COUNT(*) FROM workers"), 1);
        assert_eq!(
            w.count("SELECT COUNT(*) FROM requests"),
            0,
            "the spawn of fleetd sends no request"
        );

        w.store
            .conn()
            .execute(
                "UPDATE attempts SET via = 'claim' WHERE id = ?1",
                [&attempt],
            )
            .unwrap();
        let err = prepare(&w.ctx(None), &again, None).err().unwrap();
        assert!(err.to_string().contains("began with a claim"), "{err:#}");
    }

    /// `horch result` finds its attempt by `HORCH_TASK_YAML`. The pane of
    /// an attempt that fleetd launches with `--attempt` gets the variable
    /// from these files (`spawn::launch`, the brief of the pane): the
    /// `task.yaml` of that attempt, under the fleet root, and the first
    /// line names its `START.md`.
    #[test]
    fn spawn_of_a_bound_attempt_names_its_task_yaml() {
        let mut w = world();
        let task = w.task("a", "dev");
        let attempt = attempt_of(&w, &bind(&w));

        let mut again = args("dev");
        again.attempt = Some(attempt.clone());
        let Prepared::Pane(l) = prepare(&w.ctx(None), &again, None).unwrap() else {
            panic!("--attempt opens a pane");
        };
        let push = l.push.unwrap();
        let dir = w.fleet_root().join("tasks").join(&task).join("attempts/A1");
        assert_eq!(push.files.task_yaml, dir.join("task.yaml"));
        assert!(push.files.task_yaml.is_file() && push.files.start_md.is_file());
        assert_eq!(
            l.first_line,
            Some(format!(
                "Read {} and continue.",
                dir.join("inbox/START.md").display()
            ))
        );
    }
}
