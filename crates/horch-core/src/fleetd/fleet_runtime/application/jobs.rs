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
//!    and writes `result_json` and the end state.
//! 4. Each later pass calls [`observe`]. A run whose process is gone with
//!    no result ends `abandoned`, and its next run waits for its backoff.
//!    At `abandoned_run_limit` no new run starts, and 1 needs-you item
//!    names the job until [`requeue`]. A run past its deadline is stopped
//!    by [`stop`] after the commit: first a stop that the processes of its
//!    group can catch, then, `stop_grace_s` later, a kill of the group.
//!    The time limit of a run is this deadline on the fleet clock, so a
//!    sleep of the machine fires no time limit.
//!
//! Each state change goes through `transition()`.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::jobs_store::{self as store, JobRow, NewRun};
use crate::fleetd::fleet_runtime::adapters::scans::{Liveness, ProcessFact, ProcessItem};
use crate::fleetd::fleet_runtime::application::ledger::{
    created, transition, Cause, DoorError, RecordRefusal,
};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::jobs::{
    self, after_abandoned, AfterAbandoned, Kind, Outcome, Process, Seen, State,
};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, ProcessHost};
use crate::taskledger::finish::{self, land::CHECK_TIMEOUT};
use crate::taskledger::model::Actor;
use crate::taskledger::tasks;
use crate::vcs::network::{OTHER_BOUND, PUSH_BOUND};

/// The needs-you list, as jobs use it. The ideas context implements it, so
/// this context does not depend on ideas.
pub trait NeedsYouItems: RecordRefusal {
    /// Open the item `key` of `kind` about the row `entity_id` of `entity`,
    /// with `text`, in `tx`.
    fn open_item(
        &self,
        tx: &Connection,
        key: &str,
        kind: &str,
        entity: Entity,
        entity_id: &str,
        text: &str,
    ) -> Result<()>;

    /// End the open item `key` as resolved, in `tx`. `false` when no open
    /// item has that key.
    fn resolve_item(&self, tx: &Connection, key: &str) -> Result<bool>;
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
/// network calls (04 F11), and the kinds of `job_limit_s` their key in
/// `fleet.toml`. The design gives no number for the other kinds yet.
pub fn time_limit_s(kind: Kind, config: &FleetConfig) -> Option<u64> {
    let limits = &config.job_limit_s;
    match kind {
        Kind::TaskCheck => Some(CHECK_TIMEOUT.as_secs()),
        Kind::Fetch => Some(OTHER_BOUND.as_secs()),
        Kind::Push => Some(PUSH_BOUND.as_secs()),
        Kind::WorktreeAdd => Some(limits.worktree_add),
        Kind::WorktreeRemove => Some(limits.worktree_remove),
        Kind::WorktreeList => Some(limits.worktree_list),
        Kind::DocsSync => Some(limits.docs_sync),
        Kind::Conformance => Some(limits.conformance),
        Kind::FastGate
        | Kind::FullGate
        | Kind::TrialMerge
        | Kind::BundleBuild
        | Kind::Rebase
        | Kind::Rescue => None,
    }
}

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
            ctx.needs_you
                .open_item(tx, &key, jobs::NEEDS_YOU_KIND, Entity::Job, &id, &text)
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

/// The form of `result_json`.
#[derive(Serialize, Deserialize)]
struct ResultJson {
    passed: bool,
    verdict: bool,
    text: String,
}

fn result_json(o: &Outcome) -> String {
    serde_json::json!({ "passed": o.passed, "verdict": o.verdict, "text": o.text }).to_string()
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
    /// row is no longer `queued`: the process is killed. A failed record
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
    if !queued {
        return Ok(false);
    }
    let deadline = jobs::deadline(clock, job.time_limit_s);
    store::set_started(&tx, job.id, pid, pid_start, deadline)?;
    move_row(ctx, &tx, job.id, State::Running, "its process started")?;
    tx.commit()?;
    Ok(true)
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
/// waits until the row is `running` with the pid `me`, runs the executor
/// of its kind, then writes `result_json` and the end state in 1
/// transaction. A row that another process runs, or that the pass ended,
/// gets no write. Gives the outcome that it wrote, or `None`.
///
/// Phase 1 has an executor for `task_check` only. Each other kind ends
/// `failed`, with no verdict, and the result "no executor before phase
/// <n>". The time limit is the deadline of the row: the pass stops this
/// process at it.
pub fn run_job(
    ctx: &JobCtx<'_>,
    conn: &mut Connection,
    job_id: i64,
    me: u32,
) -> Result<Option<Outcome>> {
    let Some(row) = wait_for_record(conn, job_id, me)? else {
        return Ok(None);
    };
    let outcome = match row.kind {
        Kind::TaskCheck => task_check(conn, &row)?,
        kind => jobs::no_executor(kind),
    };
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let Some(now) = store::get(&tx, job_id)? else {
        return Ok(None);
    };
    if now.state != State::Running || now.pid != Some(me) {
        return Ok(None);
    }
    store::set_result(&tx, job_id, &result_json(&outcome))?;
    end(
        ctx,
        &tx,
        &now,
        outcome.state(),
        "the job process wrote its result",
    )?;
    tx.commit()?;
    Ok(Some(outcome))
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

/// The executor of `task_check`: the check command of the task, as
/// `finish` reads it, in the worktree of the task. Its exit code is the
/// verdict.
fn task_check(conn: &Connection, row: &JobRow) -> Result<Outcome> {
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
    let Some(wt) = finish::worktree_of(conn, &task.task_id)? else {
        return Ok(no_verdict(format!("{} has no worktree", task.short())));
    };
    let dir = Path::new(&wt.path);
    if !dir.is_dir() {
        return Ok(no_verdict(format!("the worktree {} is missing", wt.path)));
    }
    // The check stays in the process group of this job process, so the
    // stop of the job at its deadline also stops the check.
    let mut cmd = Command::new(horch_platform::shell::CHECK_SHELL);
    cmd.arg("-c")
        .arg(&check)
        .current_dir(dir)
        .stdin(Stdio::null());
    crate::runtime::process::scrub_child_env(&mut cmd);
    let status = match cmd.status() {
        Ok(s) => s,
        Err(e) => return Ok(no_verdict(format!("`{check}` did not start: {e}"))),
    };
    Ok(Outcome {
        passed: status.success(),
        verdict: true,
        text: format!("`{check}` in {}: exit {:?}", wt.path, status.code()),
    })
}
