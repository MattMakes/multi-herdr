//! The queue and the run of jobs (`docs/specs/fleetd/design.md`,
//! "Supervision, liveness and reconciliation": "Long work is a job. A pass
//! never waits for it" and "The kinds of job"; FM-12).
//!
//! A job is a row first, then its own process outside the pass:
//!
//! 1. Part 2 of a pass calls [`queue`]: a `queued` row, or the stored
//!    result of a pure run of the same input key.
//! 2. Part 3, after the commit, gives each row of [`due_to_start`] to
//!    [`JobRunner::start`]. It starts `<horch> job run <id>` through the
//!    launcher, then records the pid, the start time and the deadline on
//!    the fleet clock, and moves the row to `running`.
//! 3. The job process calls [`run_job`]. It runs the executor of the kind
//!    and writes its result file `<fleet root>/run/jobs/<job id>-<run>.json`.
//!    It writes no ledger row (RR-5).
//! 4. Each later pass reads the result files after its process scan
//!    ([`read_results`], part 1), ends each run from its file in its
//!    transaction before [`observe`] ([`ingest_results`], part 2), and
//!    archives the files after the commit ([`archive_results`], part 3).
//!    A command that waits for a run calls [`end_from_files`].
//!    Then [`observe`]: a run whose process is gone with
//!    no result ends `abandoned`, and its next run waits for its backoff.
//!    At `abandoned_run_limit` no new run starts, and 1 needs-you item
//!    names the job until [`requeue`]. A run past its deadline is stopped
//!    by [`stop`] after the commit: first a stop that the processes of its
//!    group can catch, then, `stop_grace_s` later, a kill of the group.
//!    The time limit of a run is this deadline on the fleet clock, so a
//!    sleep of the machine fires no time limit.
//!
//! Each state change goes through `transition()`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicI32, Ordering};

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::job_result_file::{
    self as result_file, FileRead, JobResult, Read as ResultRead,
};
use crate::fleetd::fleet_runtime::adapters::jobs_store::{self as store, JobRow, NewRun};
use crate::fleetd::fleet_runtime::adapters::scans::{Liveness, ProcessFact, ProcessItem};
use crate::fleetd::fleet_runtime::application::ledger::{
    created, record, transition, Cause, DoorError, RecordRefusal,
};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::jobs::{
    self, after_abandoned, AfterAbandoned, Kind, Outcome, Process, Seen, State,
};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::integration::adapters::executors as integration;
use crate::fleetd::task_execution::adapters::ingested_store::{
    self as ingested, ACCEPTED, REJECTED,
};
use crate::fleetd::task_execution::application::review;
use crate::fleetd::worker_profiles::adapters::docs_dir;
use crate::fleetd::worker_profiles::application::conformance::SuiteSubject;
use crate::fleetd::worker_profiles::application::docs_sync::{self, DocsSyncRunner};
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, ProcessHost};
use crate::fleetd::worker_profiles::domain::conformance::{Check, ConformanceReport};
use crate::fleetd::worktree_custody::adapters::executors;
use crate::taskledger::finish::complexity::{ToolRun, ToolRunner};
use crate::taskledger::finish::{self, land::CHECK_TIMEOUT};
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::runs;
use crate::taskledger::tasks;
use crate::taskledger::triage::apply as triage_apply;
use crate::taskledger::workers::{self, Worker};
use crate::vcs::network::{OTHER_BOUND, PUSH_BOUND};
use horch_platform::channel::User;

/// The needs-you list, as jobs use it. The ideas context implements it, so
/// this context does not depend on ideas.
pub trait NeedsYouItems: RecordRefusal {
    /// Open the item `item`, in `tx`.
    fn open_item(&self, tx: &Connection, item: &NewItem<'_>) -> Result<()>;

    /// End the open item `key` as resolved, in `tx`. `false` when no open
    /// item has that key.
    fn resolve_item(&self, tx: &Connection, key: &str) -> Result<bool>;
}

/// A needs-you item to open: its key and kind, the row it is about, and
/// its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewItem<'a> {
    pub key: &'a str,
    pub kind: &'a str,
    pub entity: Entity,
    pub entity_id: &'a str,
    pub text: &'a str,
}

/// What each use case of jobs needs.
pub struct JobCtx<'a> {
    /// The generation of the fleetd that writes, or `None` for a writer that
    /// is not fleetd (the job process, a CLI command).
    pub guard: Option<Generation>,
    pub config: &'a FleetConfig,
    pub needs_you: &'a dyn NeedsYouItems,
}

/// The key of the runs of 1 job: kind, subject and input key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobKey<'a> {
    pub kind: Kind,
    pub subject: &'a str,
    pub input_key: &'a str,
}

/// What [`queue`] and [`requeue`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Queued {
    /// A new `queued` run, with this id.
    New(i64),
    /// A run of the same kind and subject is `queued` or `running`. No
    /// row was added.
    Active(JobRow),
    /// The pure run of the same input key has a result that is used again.
    Reused(JobRow),
    /// The key is at `abandoned_run_limit`. No row was added: it waits for
    /// [`requeue`].
    Held(JobRow),
}

/// The time limit of a run of `kind`, in seconds. `task_check` has the
/// check timeout of today, `fetch` and `push` the fixed bounds of git
/// network calls (04 F11), and each other kind its `job_limit_s` key in
/// `fleet.toml`.
pub fn time_limit_s(kind: Kind, config: &FleetConfig) -> Option<u64> {
    let limits = &config.job_limit_s;
    match kind {
        Kind::TaskCheck => Some(CHECK_TIMEOUT.as_secs()),
        Kind::Fetch => Some(OTHER_BOUND.as_secs()),
        Kind::Push => Some(PUSH_BOUND.as_secs()),
        Kind::WorktreeAdd => Some(limits.worktree_add),
        Kind::WorktreeRemove => Some(limits.worktree_remove),
        Kind::WorktreeList => Some(limits.worktree_list),
        Kind::Rescue => Some(limits.rescue),
        Kind::DocsSync => Some(limits.docs_sync),
        Kind::Conformance => Some(limits.conformance),
        Kind::FastGate => Some(limits.fast_gate),
        Kind::FullGate => Some(limits.full_gate),
        Kind::TrialMerge => Some(limits.trial_merge),
        Kind::BundleBuild => Some(limits.bundle_build),
        Kind::Rebase => Some(limits.rebase),
        Kind::TriageApply => Some(TRIAGE_APPLY_LIMIT_S),
    }
}

/// The time limit of a `triage_apply` run, in seconds: a starting value.
/// 1 run can rebase, push, open a pull request, rescue and remove the
/// folder of each decision of 1 reply.
pub const TRIAGE_APPLY_LIMIT_S: u64 = 1800;

fn limit_of(kind: Kind, config: &FleetConfig) -> Result<u64> {
    time_limit_s(kind, config).with_context(|| {
        format!(
            "a {kind} job has no time limit before phase {}",
            kind.row().executor_phase
        )
    })
}

fn pass_cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// Queue a run of `key` for `repo`, in `tx`, at `clock` (part 2 of a
/// pass). See [`Queued`].
pub fn queue(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    key: &JobKey<'_>,
    repo: Option<&str>,
    clock: FleetSeconds,
) -> Result<Queued> {
    if let Some(active) = store::active(tx, key.kind, key.subject)? {
        return Ok(Queued::Active(active));
    }
    let last = store::latest(tx, key.kind, key.subject, key.input_key)?;
    let (abandoned_runs, not_before) = match &last {
        Some(r) if reused(r) => return Ok(Queued::Reused(r.clone())),
        Some(r) if r.state == State::Abandoned => {
            if jobs::held(r.abandoned_runs, ctx.config.abandoned_run_limit) {
                return Ok(Queued::Held(r.clone()));
            }
            let wait = jobs::backoff_s(&ctx.config.job_backoff_s, r.abandoned_runs);
            (r.abandoned_runs, Some(jobs::deadline(clock, wait)))
        }
        _ => (0, None),
    };
    let new = NewRun {
        kind: key.kind,
        subject: key.subject,
        repo,
        input_key: key.input_key,
        run: last.map_or(1, |r| r.run + 1),
        abandoned_runs,
        not_before,
        time_limit_s: limit_of(key.kind, ctx.config)?,
    };
    insert(ctx, tx, &new, "queued").map(Queued::New)
}

/// Whether the result of the ended run `r` is used again.
fn reused(r: &JobRow) -> bool {
    let verdict = outcome(r).is_some_and(|o| o.verdict);
    jobs::reusable(r.kind, r.state, verdict)
}

fn insert(ctx: &JobCtx<'_>, tx: &Connection, new: &NewRun<'_>, why: &str) -> Result<i64> {
    let id = store::insert(tx, new)?;
    created(tx, ctx.guard, Entity::Job, &id.to_string(), pass_cause(why))?;
    Ok(id)
}

/// The `queued` runs that may start at `clock`: their backoff has ended.
pub fn due_to_start(conn: &Connection, clock: FleetSeconds) -> Result<Vec<JobRow>> {
    store::due(conn, clock)
}

/// The processes of the `running` runs, for the process scan of part 1.
/// The key of each is the id of its run.
pub fn processes(conn: &Connection) -> Result<Vec<ProcessItem>> {
    Ok(store::running(conn)?
        .into_iter()
        .filter_map(|r| {
            Some(ProcessItem {
                key: r.id.to_string(),
                pid: r.pid?,
                pid_start: r.pid_start,
            })
        })
        .collect())
}

/// The stop of the process group of a run past its deadline: an effect
/// for part 3 of a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stop {
    pub job_id: i64,
    /// The pid of the job process, the leader of its group.
    pub pid: u32,
    pub pid_start: Option<u64>,
    /// Past the deadline plus `stop_grace_s`: kill the group.
    pub kill: bool,
}

/// What [`observe`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed {
    /// The runs that ended, each with its end state.
    pub ended: Vec<(i64, State)>,
    /// The stops to do after the commit.
    pub stops: Vec<Stop>,
}

/// Observe each `running` run with the process `facts` of part 1, in `tx`,
/// at `clock` (part 2 of a pass). A run with no fact in `facts` waits for
/// a later pass, unless it wrote its result. A refused move fails only its
/// run.
pub fn observe(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    facts: &[ProcessFact],
    clock: FleetSeconds,
) -> Result<Observed> {
    let mut seen = Observed::default();
    for row in store::running(tx)? {
        let key = row.id.to_string();
        let process = match facts.iter().find(|f| f.key == key).map(|f| f.liveness) {
            Some(Liveness::Alive) => Process::Alive,
            Some(Liveness::Dead) => Process::Gone,
            Some(Liveness::Unknown) | None => Process::Unknown,
        };
        let result = outcome(&row).map(|o| o.state());
        let deadline = row.deadline.unwrap_or(FleetSeconds(i64::MAX));
        let stop_grace_s = ctx.config.stop_grace_s;
        let state = match jobs::observe(process, result, deadline, stop_grace_s, clock) {
            Seen::Runs => continue,
            Seen::Stop { kill } => {
                if let Some(pid) = row.pid {
                    seen.stops.push(Stop {
                        job_id: row.id,
                        pid,
                        pid_start: row.pid_start,
                        kill,
                    });
                }
                continue;
            }
            Seen::Ends(state) => state,
        };
        let done = if state == State::Abandoned {
            abandon(ctx, tx, &row, clock)
        } else {
            end(ctx, tx, &row, state, "its process wrote its result")
        };
        match done {
            Ok(()) => seen.ended.push((row.id, state)),
            Err(e) if is_refused(&e) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(seen)
}

fn is_refused(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<DoorError>(),
        Some(DoorError::Refused { .. })
    )
}

/// Move the run `row` to its end `state`, a passed or failed state.
fn end(ctx: &JobCtx<'_>, tx: &Connection, row: &JobRow, state: State, why: &str) -> Result<()> {
    move_row(ctx, tx, row.id, state, why)?;
    store::set_ended(tx, row.id, 0)
}

/// End the run `row` `abandoned`, and queue its next run after its
/// backoff, or hold its key with 1 needs-you item.
fn abandon(ctx: &JobCtx<'_>, tx: &Connection, row: &JobRow, clock: FleetSeconds) -> Result<()> {
    move_row(
        ctx,
        tx,
        row.id,
        State::Abandoned,
        "its process is gone with no result",
    )?;
    let c = ctx.config;
    match after_abandoned(
        row.abandoned_runs,
        c.abandoned_run_limit,
        &c.job_backoff_s,
        clock,
    ) {
        AfterAbandoned::Again {
            abandoned_runs,
            not_before,
        } => {
            store::set_ended(tx, row.id, abandoned_runs)?;
            let next = NewRun {
                kind: row.kind,
                subject: &row.subject,
                repo: row.repo.as_deref(),
                input_key: &row.input_key,
                run: row.run + 1,
                abandoned_runs,
                not_before: Some(not_before),
                time_limit_s: row.time_limit_s,
            };
            insert(ctx, tx, &next, "the run before it ended abandoned").map(drop)
        }
        AfterAbandoned::Held { abandoned_runs } => {
            store::set_ended(tx, row.id, abandoned_runs)?;
            let key = jobs::needs_you_key(row.kind, &row.subject, &row.input_key);
            let text = format!(
                "Job {id} ({kind} of {subject}, input key {input}) ended abandoned \
                 {abandoned_runs} times in a row, so no new run starts. \
                 Run: horch job requeue {id}",
                id = row.id,
                kind = row.kind,
                subject = row.subject,
                input = row.input_key,
            );
            let id = row.id.to_string();
            let item = NewItem {
                key: &key,
                kind: jobs::NEEDS_YOU_KIND,
                entity: Entity::Job,
                entity_id: &id,
                text: &text,
            };
            ctx.needs_you.open_item(tx, &item)
        }
    }
}

fn move_row(ctx: &JobCtx<'_>, tx: &Connection, id: i64, to: State, why: &str) -> Result<()> {
    let refusals: &dyn RecordRefusal = ctx.needs_you;
    let id = id.to_string();
    transition(
        tx,
        ctx.guard,
        Entity::Job,
        &id,
        to.as_str(),
        pass_cause(why),
        refusals,
    )?;
    Ok(())
}

/// `horch job requeue`: queue 1 more run of `key` with its count of
/// abandoned runs at 0, and resolve its needs-you item, in `tx`. A run of
/// the same kind and subject that is `queued` or `running` gives
/// [`Queued::Active`].
pub fn requeue(ctx: &JobCtx<'_>, tx: &Connection, key: &JobKey<'_>) -> Result<Queued> {
    if let Some(active) = store::active(tx, key.kind, key.subject)? {
        return Ok(Queued::Active(active));
    }
    let Some(last) = store::latest(tx, key.kind, key.subject, key.input_key)? else {
        bail!(
            "no {} job of {} with input key {}",
            key.kind,
            key.subject,
            key.input_key
        );
    };
    let new = NewRun {
        kind: key.kind,
        subject: key.subject,
        repo: last.repo.as_deref(),
        input_key: key.input_key,
        run: last.run + 1,
        abandoned_runs: 0,
        not_before: None,
        time_limit_s: limit_of(key.kind, ctx.config)?,
    };
    let id = store::insert(tx, &new)?;
    let cause = Cause {
        actor: Actor::Operator,
        text: "horch job requeue",
    };
    created(tx, ctx.guard, Entity::Job, &id.to_string(), cause)?;
    let item = jobs::needs_you_key(key.kind, key.subject, key.input_key);
    ctx.needs_you.resolve_item(tx, &item)?;
    Ok(Queued::New(id))
}

/// The stored result of the run `row`, when it wrote one.
pub fn outcome(row: &JobRow) -> Option<Outcome> {
    let r: ResultJson = serde_json::from_str(row.result_json.as_deref()?).ok()?;
    Some(Outcome {
        passed: r.passed,
        verdict: r.passed || r.verdict,
        text: r.text,
    })
}

/// The facts that the run `row` wrote beside its outcome, when its
/// executor gives facts: for example the path, the branch and the HEAD of
/// a `worktree_add` run.
pub fn result_data(row: &JobRow) -> Option<serde_json::Value> {
    let r: ResultJson = serde_json::from_str(row.result_json.as_deref()?).ok()?;
    r.data
}

/// What an executor gives: the outcome of the run, and the facts of its
/// result for the reader of the run.
#[derive(Debug, Clone, PartialEq)]
pub struct Ran {
    pub outcome: Outcome,
    pub data: Option<serde_json::Value>,
}

impl From<Outcome> for Ran {
    fn from(outcome: Outcome) -> Ran {
        Ran {
            outcome,
            data: None,
        }
    }
}

/// The form of `result_json`.
#[derive(Serialize, Deserialize)]
struct ResultJson {
    passed: bool,
    verdict: bool,
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

fn result_json(ran: &Ran) -> String {
    let o = &ran.outcome;
    let r = ResultJson {
        passed: o.passed,
        verdict: o.verdict,
        text: o.text.clone(),
        data: ran.data.clone(),
    };
    serde_json::to_string(&r).expect("a result serializes")
}

/// The kinds that change the git state of their repository. At most 1 run
/// of them runs for 1 repository at a time (MAP.md of fleetd-p3, Q12;
/// design, "1 git writer for each repository").
const GIT_WRITERS: [Kind; 8] = [
    Kind::WorktreeAdd,
    Kind::WorktreeRemove,
    Kind::Rescue,
    Kind::BundleBuild,
    Kind::TriageApply,
    Kind::Rebase,
    Kind::Fetch,
    Kind::Push,
];

/// Whether a run of `kind` changes the git state of its repository.
pub fn is_git_writer(kind: Kind) -> bool {
    GIT_WRITERS.contains(&kind)
}

/// The `running` runs of the git writers of the repository `repo`.
pub fn git_writers_running(conn: &Connection, repo: &str) -> Result<Vec<JobRow>> {
    Ok(store::running(conn)?
        .into_iter()
        .filter(|r| is_git_writer(r.kind) && r.repo.as_deref() == Some(repo))
        .collect())
}

/// Whether a run of a git writer of the repository `repo` is `running`
/// ("1 git writer for each repository").
pub fn git_writer_busy(conn: &Connection, repo: &str) -> Result<bool> {
    Ok(!git_writers_running(conn, repo)?.is_empty())
}

/// The launch of the job process of the run `job_id`: `<horch> job run
/// <job_id>`, with its output appended to `output`.
pub fn job_spec(horch: &Path, job_id: i64, output: Option<PathBuf>) -> LaunchSpec {
    LaunchSpec {
        program: horch.to_path_buf(),
        args: vec!["job".into(), "run".into(), job_id.to_string().into()],
        cwd: None,
        env_rule: Default::default(),
        detach: true,
        output,
    }
}

/// The starts of job processes (part 3 of a pass). It keeps each started
/// process to reap it, so that an ended job process is gone, not a zombie,
/// for the next process scan.
pub struct JobRunner<'a> {
    host: &'a dyn ProcessHost,
    children: Vec<Child>,
}

impl<'a> JobRunner<'a> {
    pub fn new(host: &'a dyn ProcessHost) -> Self {
        JobRunner {
            host,
            children: Vec::new(),
        }
    }

    /// Start the `queued` run `job` as `spec`, after the commit of the
    /// pass, at `clock`. Then record its process and its deadline, and
    /// move it to `running`, in 1 transaction of `conn`. `false` when the
    /// row is no longer `queued`, or when it is a git writer and another git
    /// writer of its repository runs: the process is killed. A failed record
    /// also kills the process.
    pub fn start(
        &mut self,
        ctx: &JobCtx<'_>,
        conn: &mut Connection,
        job: &JobRow,
        spec: &LaunchSpec,
        clock: FleetSeconds,
    ) -> Result<bool> {
        let launched = self
            .host
            .launch(spec)
            .with_context(|| format!("starting job {}", job.id))?;
        let mut child = launched.child;
        let recorded = record_start(ctx, conn, job, launched.pid, launched.pid_start, clock);
        if !matches!(recorded, Ok(true)) {
            horch_platform::group::kill_child_group(&mut child);
            let _ = child.wait();
            return recorded;
        }
        self.children.push(child);
        Ok(true)
    }

    /// Reap each started process that ended. Call it before the process
    /// scan.
    pub fn reap(&mut self) {
        self.children
            .retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_)) | Err(_)));
    }
}

fn record_start(
    ctx: &JobCtx<'_>,
    conn: &mut Connection,
    job: &JobRow,
    pid: u32,
    pid_start: Option<u64>,
    clock: FleetSeconds,
) -> Result<bool> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let queued = store::get(&tx, job.id)?.is_some_and(|r| r.state == State::Queued);
    if !queued || writer_waits(&tx, job)? {
        return Ok(false);
    }
    let deadline = jobs::deadline(clock, job.time_limit_s);
    store::set_started(&tx, job.id, pid, pid_start, deadline)?;
    move_row(ctx, &tx, job.id, State::Running, "its process started")?;
    tx.commit()?;
    Ok(true)
}

/// Whether the git-writing run `job` must wait: another git writer of its
/// repository runs. The check is in the transaction of the move to
/// `running`, so 2 starters never run 2 writers of 1 repository.
fn writer_waits(tx: &Connection, job: &JobRow) -> Result<bool> {
    match &job.repo {
        Some(repo) if is_git_writer(job.kind) => git_writer_busy(tx, repo),
        _ => Ok(false),
    }
}

/// Stop the process group of a run past its deadline (part 3 of a pass),
/// only while its leader, or a member of its group, holds the recorded
/// start. Before the kill, each process of the group gets a stop that it
/// can catch. Whether a stop was sent.
///
/// Windows has no process groups: there the stop reaches the job process
/// only, and the processes that it started live on
/// (`docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
/// row "Process groups").
pub fn stop(s: &Stop) -> bool {
    use horch_platform::group;
    use horch_platform::signal::{stop_same, Stop as How};
    let Some(start) = s.pid_start else {
        return false;
    };
    let how = if s.kill { How::Kill } else { How::Term };
    match group::is_same_group(s.pid, start) {
        Ok(false) => false,
        Err(_) => stop_same(s.pid, start, how),
        Ok(true) if s.kill => group::kill_group_id(s.pid).unwrap_or(false),
        Ok(true) => {
            let members =
                horch_platform::process::group_members(s.pid).unwrap_or_else(|_| vec![s.pid]);
            let mut sent = false;
            for pid in members {
                if let Some(t) = horch_platform::process::start_time(pid) {
                    sent |= stop_same(pid, t, how);
                }
            }
            sent
        }
    }
}

/// The time to wait for the starter of a job process to record it.
const RECORD_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// `horch job run <id>`: the job process `me` of the run `job_id`. It
/// reads its row only: it waits until the row is `running` with the pid
/// `me`, runs the executor of its kind, then writes its result file
/// atomically under the fleet root of `conn` (Q14 of
/// `docs/tickets/fleetd-p5/MAP.md`). The pass ends the run from the file.
/// A row that another process runs, or that the pass ended, gets no file.
/// Gives the outcome that it wrote, or `None`.
///
/// `task_check`, `docs_sync` and `conformance` have their executors here,
/// the 4 worktree kinds have theirs in worktree custody (MAP.md of
/// fleetd-p3, Q22), and the 7 integration kinds have their router in the
/// integration context. A `docs_sync` or `conformance` run with no runner
/// ends `failed`, with no verdict, and the result "no executor before phase
/// <n>". The time limit is the deadline of the
/// row: the pass stops this process at it. `checks` starts a check (SR-4).
/// The job process writes no row, so it uses no field of `_ctx`; the
/// parameter stays so that the callers of Phase 4 keep their form.
///
/// This form has no docs runner and no suite, so `docs_sync` and
/// `conformance` have no executor here; `horch job run` calls
/// [`run_job_with`].
pub fn run_job(
    ctx: &JobCtx<'_>,
    conn: &Connection,
    job_id: i64,
    me: u32,
    checks: &CheckRunner<'_>,
) -> Result<Option<Outcome>> {
    let runners = Runners {
        checks,
        docs: None,
        conformance: None,
    };
    run_job_with(ctx, conn, job_id, me, &runners)
}

/// The executors that need a runner from outside horch-core: the check of
/// a task (SR-4), the fetch of the docs (NFR-06) and the conformance suite
/// of a CLI.
pub struct Runners<'a> {
    pub checks: &'a CheckRunner<'a>,
    /// `None`: a `docs_sync` run ends with "no executor".
    pub docs: Option<&'a dyn DocsSyncRunner>,
    /// `None`: a `conformance` run ends with "no executor".
    pub conformance: Option<&'a dyn ConformanceSuite>,
}

/// The port of the conformance suite of 1 CLI (step 3 of 1 docs sync).
/// `horch` gives it with the real CLI binary and the reference profile of
/// the CLI.
pub trait ConformanceSuite {
    /// The `version_cmd` of the source `cli` of `docs-sources.toml`. An
    /// unknown `cli` is an error that names it.
    fn version_cmd(&self, cli: &str) -> Result<String>;

    /// The hash of the adapter of `cli`: the sha256 of its rendered
    /// conformance launch for the reference profile (MAP.md of fleetd-p7,
    /// Q9).
    fn adapter_hash(&self, cli: &str) -> Result<String>;

    /// Run each check of the suite of `subject`.
    fn run(&self, subject: SuiteSubject) -> Result<ConformanceReport>;
}

/// [`run_job`] with each runner of `runners`.
pub fn run_job_with(
    _ctx: &JobCtx<'_>,
    conn: &Connection,
    job_id: i64,
    me: u32,
    runners: &Runners<'_>,
) -> Result<Option<Outcome>> {
    let Some(row) = wait_for_record(conn, job_id, me)? else {
        return Ok(None);
    };
    let ran = match row.kind {
        Kind::TaskCheck if review::is_complexity_key(&row.input_key) => {
            complexity_sweep(conn, &row, runners.checks)?
        }
        Kind::TaskCheck => task_check(conn, &row, runners.checks)?.into(),
        Kind::DocsSync => match runners.docs {
            Some(docs) => docs_sync_run(conn, &row, runners.checks.host, docs)?,
            None => jobs::no_executor(row.kind).into(),
        },
        Kind::Conformance => match runners.conformance {
            Some(suite) => conformance_run(conn, &row, runners.checks.host, suite)?,
            None => jobs::no_executor(row.kind).into(),
        },
        Kind::WorktreeAdd | Kind::WorktreeList | Kind::Rescue | Kind::WorktreeRemove => {
            executors::run(conn, &row)?
        }
        Kind::BundleBuild
        | Kind::FastGate
        | Kind::FullGate
        | Kind::Fetch
        | Kind::Push
        | Kind::Rebase
        | Kind::TrialMerge => integration::run(conn, &row)?,
        Kind::TriageApply => triage_apply::run(conn, &row)?,
    };
    let Some(now) = store::get(conn, job_id)? else {
        return Ok(None);
    };
    if now.state != State::Running || now.pid != Some(me) {
        return Ok(None);
    }
    let o = &ran.outcome;
    let file = JobResult {
        job_id,
        run: now.run,
        passed: o.passed,
        verdict: o.verdict,
        text: o.text.clone(),
        data: ran.data.clone(),
    };
    result_file::write(&executors::fleet_root(conn)?, &file)?;
    Ok(Some(ran.outcome))
}

/// The cause of the end of a run from its result file.
const WROTE_ITS_RESULT: &str = "the job process wrote its result";

/// Part 1 of a pass, after the process scan: read each result file under
/// `fleet_root` with the owner `owner` and the size limit `max_bytes`
/// (SR-6). A path with an `ingested` row is not read again.
pub fn read_results(
    conn: &Connection,
    fleet_root: &Path,
    owner: &User,
    max_bytes: u64,
) -> Result<Vec<FileRead>> {
    let mut seen = |path: &Path| ingested::has(conn, &ingested_key(path));
    result_file::read_folder(fleet_root, owner, max_bytes, &mut seen)
}

/// Part 2 of a pass, before [`observe`]: record each file of `reads` 1
/// time in `ingested`, in `tx`, and end each run from its valid file. A
/// valid file of a run that is not `running`, or whose run is not the run
/// of its row, is ignored with 1 event. Gives the runs that ended, each
/// with its end state.
pub fn ingest_results(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    reads: &[FileRead],
) -> Result<Vec<(i64, State)>> {
    let at = crate::taskledger::util::now();
    let mut ended = Vec::new();
    for file in reads {
        let key = ingested_key(&file.path);
        let (sha256, result) = match &file.read {
            ResultRead::Seen => continue,
            ResultRead::Rejected { sha256, reason } => {
                let sha256 = sha256.as_deref();
                ingested::record_message(tx, &key, sha256, REJECTED, Some(reason), &at)?;
                continue;
            }
            ResultRead::Valid { sha256, result } => (sha256.as_str(), result),
        };
        let Some(row) = store::get(tx, result.job_id)? else {
            let reason = format!("no job {}", result.job_id);
            ingested::record_message(tx, &key, Some(sha256), REJECTED, Some(&reason), &at)?;
            continue;
        };
        if !ingested::record_message(tx, &key, Some(sha256), ACCEPTED, None, &at)? {
            continue;
        }
        if let Some(state) = end_from(ctx, tx, &row, result)? {
            ended.push((row.id, state));
        }
    }
    Ok(ended)
}

/// End the run `row` from its result file `result`, in `tx`: its end
/// state, or `None` when the file is not of the current run of the row.
fn end_from(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    row: &JobRow,
    result: &JobResult,
) -> Result<Option<State>> {
    if row.run != result.run || row.state != State::Running {
        let text = format!(
            "ignored the result file of run {} of job {}: the row is run {} and {}",
            result.run,
            row.id,
            row.run,
            row.state.as_str()
        );
        let data = serde_json::json!({ "run": result.run, "row_run": row.run });
        let id = row.id.to_string();
        record(
            tx,
            ctx.guard,
            EventKind::Note,
            Entity::Job,
            &id,
            pass_cause(&text),
            &data,
        )?;
        return Ok(None);
    }
    let ran = Ran {
        outcome: Outcome {
            passed: result.passed,
            verdict: result.passed || result.verdict,
            text: result.text.clone(),
        },
        data: result.data.clone(),
    };
    let ran = record_facts(ctx, tx, row, ran)?;
    let state = ran.outcome.state();
    store::set_result(tx, row.id, &result_json(&ran))?;
    end(ctx, tx, row, state, WROTE_ITS_RESULT)?;
    Ok(Some(state))
}

/// Write the facts of the result `ran` of the run `row`, in `tx`, before
/// its end (RR-5: the job process writes no row). A refused fact rolls back
/// each write of the facts, and the run ends `failed` with the reason and
/// no facts.
fn record_facts(ctx: &JobCtx<'_>, tx: &Connection, row: &JobRow, ran: Ran) -> Result<Ran> {
    tx.execute_batch("SAVEPOINT job_facts")?;
    let recorded = match row.kind {
        Kind::TriageApply => triage_apply::ingest(ctx, tx, row, &ran),
        _ => integration::record(ctx, tx, row, &ran),
    };
    match recorded {
        Ok(()) => {
            tx.execute_batch("RELEASE job_facts")?;
            Ok(ran)
        }
        Err(why) => {
            tx.execute_batch("ROLLBACK TO job_facts; RELEASE job_facts")?;
            Ok(Outcome {
                passed: false,
                verdict: false,
                text: format!("fleetd refused the facts of the result file: {why:#}"),
            }
            .into())
        }
    }
}

/// Part 3 of a pass, after the commit: move each file of `reads` to
/// `run/jobs/archive/`. A second archive of the same reads is safe.
pub fn archive_results(fleet_root: &Path, reads: &[FileRead]) -> Result<()> {
    for file in reads {
        result_file::archive(fleet_root, &file.path)?;
    }
    Ok(())
}

/// The 3 parts of the result files for a command that waits for a run
/// with no pass (`run_now`): read the files under the fleet root of
/// `conn`, end each run from its file in 1 transaction, then archive the
/// files. The owner is the user of this process. Call it after the process
/// scan and before [`observe`], so that a process that wrote its file and
/// ended is not seen as gone with no result. Gives the runs that ended.
pub fn end_from_files(ctx: &JobCtx<'_>, conn: &mut Connection) -> Result<Vec<(i64, State)>> {
    let root = executors::fleet_root(conn)?;
    let owner = User::current().context("the user of this process")?;
    let reads = read_results(conn, &root, &owner, ctx.config.job_result_max_bytes)?;
    if reads.is_empty() {
        return Ok(Vec::new());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let ended = ingest_results(ctx, &tx, &reads)?;
    tx.commit()?;
    archive_results(&root, &reads)?;
    Ok(ended)
}

/// The `ingested.path` of a result file.
fn ingested_key(path: &Path) -> String {
    path.display().to_string()
}

/// The row `job_id` once it is `running` with the pid `me`, or `None` when
/// another state or pid holds it, or after [`RECORD_WAIT`].
fn wait_for_record(conn: &Connection, job_id: i64, me: u32) -> Result<Option<JobRow>> {
    let until = std::time::Instant::now() + RECORD_WAIT;
    loop {
        let row = store::get(conn, job_id)?.with_context(|| format!("no job {job_id}"))?;
        match row.state {
            State::Running if row.pid == Some(me) => return Ok(Some(row)),
            State::Queued if std::time::Instant::now() < until => {}
            _ => return Ok(None),
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// The executor of `docs_sync`: steps 1 and 2 of 1 sync of the CLI of the
/// subject `<cli> <binary id>` (MAP.md of fleetd-p7, Q8) into
/// `<fleet root>/docs/`. The run has no repository. Its result holds the
/// version text, the folder and each failed page; it passes when each page
/// and CHANGES.md are written.
fn docs_sync_run(
    conn: &Connection,
    row: &JobRow,
    host: &dyn ProcessHost,
    docs: &dyn DocsSyncRunner,
) -> Result<Ran> {
    let cli = row.subject.split_whitespace().next().unwrap_or_default();
    let out = docs_dir::docs_root(&executors::fleet_root(conn)?);
    Ok(match docs_sync::sync_cli(host, docs, cli, &out) {
        Ok(run) => Ran {
            outcome: Outcome {
                passed: run.passed(),
                verdict: true,
                text: run.summary(),
            },
            data: Some(serde_json::to_value(&run)?),
        },
        Err(e) => Outcome {
            passed: false,
            verdict: false,
            text: format!("docs sync of {cli}: {e:#}"),
        }
        .into(),
    })
}

/// The facts of the result of a `conformance` run.
#[derive(Serialize)]
struct ConformanceData<'a> {
    cli: &'a str,
    version: &'a str,
    adapter_hash: &'a str,
    /// `<fleet root>/docs/<cli>/<version>/conformance.json`.
    path: PathBuf,
    /// The name of each failed check.
    failed: Vec<&'static str>,
}

/// The executor of `conformance`: step 3 of 1 sync of the CLI of the
/// subject `<cli> <binary id>` (MAP.md of fleetd-p7, Q8). The version text
/// comes from the version command, started through `host` (SR-7). The
/// suite runs, and its report goes to
/// `<fleet root>/docs/<cli>/<version>/conformance.json`. The run has no
/// repository. It passes when no check fails; a check that is `not
/// supported` does not fail it. A failed run names each failed check.
fn conformance_run(
    conn: &Connection,
    row: &JobRow,
    host: &dyn ProcessHost,
    suite: &dyn ConformanceSuite,
) -> Result<Ran> {
    let cli = row.subject.split_whitespace().next().unwrap_or_default();
    let out = docs_dir::docs_root(&executors::fleet_root(conn)?);
    let run = || -> Result<(ConformanceReport, PathBuf)> {
        let cmd = suite.version_cmd(cli)?;
        let version =
            docs_sync::version_text(host, &cmd, &out).with_context(|| format!("`{cmd}`"))?;
        let adapter_hash = suite
            .adapter_hash(cli)
            .context("rendering the conformance launch")?;
        let report = suite.run(SuiteSubject {
            cli: cli.to_string(),
            version,
            adapter_hash,
        })?;
        let path = docs_dir::write_conformance(&out, &report)?;
        Ok((report, path))
    };
    let (report, path) = match run() {
        Ok(done) => done,
        Err(e) => {
            return Ok(Outcome {
                passed: false,
                verdict: false,
                text: format!("conformance of {cli}: {e:#}"),
            }
            .into())
        }
    };
    let failure = report.failure();
    let text = match &failure {
        Some(failure) => failure.to_string(),
        None => format!(
            "conformance of {} {}: each check passed",
            report.cli, report.version
        ),
    };
    let data = ConformanceData {
        cli: &report.cli,
        version: &report.version,
        adapter_hash: &report.adapter_hash,
        path,
        failed: report.failed().into_iter().map(Check::name).collect(),
    };
    Ok(Ran {
        outcome: Outcome {
            passed: failure.is_none(),
            verdict: true,
            text,
        },
        data: Some(serde_json::to_value(&data)?),
    })
}

/// The executor of `task_check`: the check command of the task, as
/// `finish` reads it, in the worktree of the task (the project root for a
/// task of the shared checkout), in the sandbox profile of the worker of
/// the task (SR-4). Its exit code is the verdict. A task
/// with no worker, or a worker with no sandbox profile, gets no check.
fn task_check(conn: &Connection, row: &JobRow, checks: &CheckRunner<'_>) -> Result<Outcome> {
    let no_verdict = |text: String| Outcome {
        passed: false,
        verdict: false,
        text,
    };
    let task = tasks::get(conn, &row.subject)?;
    let default = finish::default_check(conn, &task.run_id)?;
    let Some(check) = task
        .land_check
        .clone()
        .or(default)
        .filter(|c| !c.trim().is_empty())
    else {
        if task.check_none {
            return Ok(Outcome {
                passed: true,
                verdict: true,
                text: "the ticket says Check: none".into(),
            });
        }
        return Ok(no_verdict(format!("{} has no check command", task.short())));
    };
    // A task of the shared checkout works in the project root.
    let dir = if task.uses_worktree() {
        let Some(wt) = finish::worktree_of(conn, &task.task_id)? else {
            return Ok(no_verdict(format!("{} has no worktree", task.short())));
        };
        PathBuf::from(wt.path)
    } else {
        PathBuf::from(runs::get(conn, &task.run_id)?.project_root)
    };
    if !dir.is_dir() {
        return Ok(no_verdict(format!(
            "the folder {} is missing",
            dir.display()
        )));
    }
    let dir = dir.as_path();
    let Some(worker_id) = task.current_worker_id.as_deref() else {
        return Ok(no_verdict(format!(
            "SR-4: {} has no worker, so its check has no sandbox profile; \
             a check never runs with the rights of fleetd",
            task.short()
        )));
    };
    let worker = workers::get(conn, worker_id)?;
    let Some(spec) = checks.profiles.check_launch(&worker, dir, &check)? else {
        return Ok(no_verdict(format!(
            "SR-4: worker {worker_id} of {} has no sandbox profile; \
             a check never runs with the rights of fleetd",
            task.short()
        )));
    };
    let status = match run_check(checks.host, &spec) {
        Ok(s) => s,
        Err(e) => return Ok(no_verdict(format!("`{check}` did not start: {e:#}"))),
    };
    Ok(Outcome {
        passed: status.success(),
        verdict: true,
        text: format!("`{check}` in {}: exit {:?}", dir.display(), status.code()),
    })
}

fn complexity_sweep(conn: &Connection, row: &JobRow, checks: &CheckRunner<'_>) -> Result<Ran> {
    let no_verdict = |text: String| {
        Ran::from(Outcome {
            passed: false,
            verdict: false,
            text,
        })
    };
    let Some(tools) = checks.sweep else {
        return Ok(no_verdict(
            "this job process has no tools of the finish steps".into(),
        ));
    };
    let task = tasks::get(conn, &row.subject)?;
    let Some(worker_id) = task.current_worker_id.as_deref() else {
        return Ok(no_verdict(format!(
            "SR-4: {} has no worker, so its complexity-sweep has no sandbox profile; \
             a check never runs with the rights of fleetd",
            task.short()
        )));
    };
    let worker = workers::get(conn, worker_id)?;
    let att = match finish::VerifierAttempt::open(conn, tools, &task.task_id, worker_id) {
        Ok(a) => a,
        Err(e) => return Ok(no_verdict(format!("complexity-sweep: {e:#}"))),
    };
    if checks
        .profiles
        .check_launch(&worker, att.dir(), "true")?
        .is_none()
    {
        return Ok(no_verdict(format!(
            "SR-4: worker {worker_id} of {} has no sandbox profile; \
             a check never runs with the rights of fleetd",
            task.short()
        )));
    }
    let runner = WorkerTools {
        checks,
        worker: &worker,
    };
    let o = match att.complexity_sweep(conn, &runner) {
        Ok(o) => o,
        Err(e) => return Ok(no_verdict(format!("complexity-sweep: {e:#}"))),
    };
    Ok(Ran {
        outcome: Outcome {
            passed: o.status.is_through(),
            verdict: true,
            text: format!("complexity-sweep: {}", o.note),
        },
        data: Some(review::step_data(&o)),
    })
}

struct WorkerTools<'a> {
    checks: &'a CheckRunner<'a>,
    worker: &'a Worker,
}

impl ToolRunner for WorkerTools<'_> {
    fn run(&self, program: &Path, args: &[OsString], dir: &Path, stdout: &Path) -> Result<ToolRun> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let marker = format!("horch-stderr-{}-{nanos}", std::process::id());
        let script = tool_script(program, args, &marker);
        let Some(mut spec) = self
            .checks
            .profiles
            .check_launch(self.worker, dir, &script)?
        else {
            bail!(
                "SR-4: worker {} has no sandbox profile; a check never runs with the rights of fleetd",
                self.worker.worker_id
            );
        };
        std::fs::File::create(stdout)?;
        spec.output = Some(stdout.to_path_buf());
        let status = run_check(self.checks.host, &spec)?;
        let all = std::fs::read(stdout)?;
        let (out, err) = split_stderr(&all, &marker);
        std::fs::write(stdout, out)?;
        Ok(ToolRun {
            success: status.success(),
            stderr: String::from_utf8_lossy(err).into_owned(),
        })
    }
}

/// The temp file is in `$TMPDIR`, which each sandbox profile can write:
/// macOS `mktemp` with no template ignores `$TMPDIR`.
fn tool_script(program: &Path, args: &[OsString], marker: &str) -> String {
    let quote = |w: &std::ffi::OsStr| format!("'{}'", w.to_string_lossy().replace('\'', r"'\''"));
    let words: Vec<String> = std::iter::once(program.as_os_str())
        .chain(args.iter().map(OsString::as_os_str))
        .map(quote)
        .collect();
    format!(
        "e=$(mktemp \"${{TMPDIR:-/tmp}}/horch-stderr.XXXXXX\") || exit 125\n{} 2>\"$e\"\ns=$?\nprintf '\\n%s\\n' '{marker}'\ncat \"$e\"\nrm -f \"$e\"\nexit \"$s\"\n",
        words.join(" ")
    )
}

fn split_stderr<'b>(all: &'b [u8], marker: &str) -> (&'b [u8], &'b [u8]) {
    let sep = format!("\n{marker}\n");
    match all.windows(sep.len()).position(|w| w == sep.as_bytes()) {
        Some(i) => (&all[..i], &all[i + sep.len()..]),
        None => (&[], all),
    }
}

/// The sandbox profiles of workers, as the check of a task needs them
/// (SR-4). `worker_profiles::adapters::check_sandbox` implements it.
pub trait SandboxProfiles {
    /// The launch of `check` in `dir` in the sandbox profile of `worker`,
    /// or `None` when the worker has no sandbox profile.
    fn check_launch(&self, worker: &Worker, dir: &Path, check: &str) -> Result<Option<LaunchSpec>>;
}

/// What starts a check: the launcher and the sandbox profiles.
pub struct CheckRunner<'a> {
    pub host: &'a dyn ProcessHost,
    pub profiles: &'a dyn SandboxProfiles,
    /// The tools of the finish steps, for the complexity-sweep of a
    /// `task_check` job (ticket 20 of fleetd-p4). `None`: that job gets no
    /// verdict.
    pub sweep: Option<&'a finish::Tools>,
}

/// The group of the running check, 0 when none runs.
static CHECK_GROUP: AtomicI32 = AtomicI32::new(0);

/// Start `spec` through `host` and wait for it. The check leads its own
/// group. A stop of this job process at its deadline (`SIGTERM`) kills
/// that group first, so no check outlives its job. Windows has no signal
/// handlers: there the check lives on after the stop
/// (`docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
/// row "Signals and stop").
fn run_check(host: &dyn ProcessHost, spec: &LaunchSpec) -> Result<std::process::ExitStatus> {
    let mut launched = host.launch(spec)?;
    let group = i32::try_from(launched.pid).unwrap_or(0);
    CHECK_GROUP.store(group, Ordering::SeqCst);
    let _ = horch_platform::signal::kill_group_on_termination(&CHECK_GROUP);
    let status = launched.child.wait();
    CHECK_GROUP.store(0, Ordering::SeqCst);
    Ok(status?)
}
