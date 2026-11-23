use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};

use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::{created, transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::ideas::application::park_cancel::PARKED;
use crate::fleetd::ideas::application::questions;
use crate::fleetd::integration::application::landing;
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::task_execution::adapters::sqlite as store;
use crate::fleetd::task_execution::adapters::task_files::{
    attempt_dir, write_attempt_files_with, AttemptFiles,
};
use crate::fleetd::task_execution::application::{outbox_ingest, repair, review};
use crate::fleetd::task_execution::domain::dispatch::{
    choose, Choice, Hold, Holds, ReadyTask, RunningTask,
};
use crate::fleetd::task_execution::domain::task::{
    attempt_id, judged_task, AttemptRow, AttemptState,
};
use crate::fleetd::task_execution::domain::task_files::{
    answers_section, AnsweredQuestion, Secret, TaskFacts,
};
use crate::fleetd::worker_profiles::application::health;
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::application::create::{create_worktree, CreateOutcome, Lease};
use crate::taskledger::git::Git;
use crate::taskledger::model::{Actor, FileMode, TaskStatus, WorkerKind, WorktreeStatus};
use crate::taskledger::runs::{self, Run};
use crate::taskledger::sql_words::{word, WORKTREES_GONE};
use crate::taskledger::store::one_snapshot;
use crate::taskledger::tasks::{self, Task};
use crate::taskledger::util;
use crate::taskledger::workers::{self, NewWorker};

pub trait CheckSandboxes {
    fn supported(&self, persona: &str) -> bool;
}

pub const NO_CHECK_SANDBOX: &str =
    "no check sandbox on this host: its check cannot run in the worker sandbox (SR-4)";

const HOLDING: [TaskStatus; 6] = [
    TaskStatus::Leased,
    TaskStatus::Running,
    TaskStatus::Stalled,
    TaskStatus::Verifying,
    TaskStatus::BlockedOnUser,
    TaskStatus::WaitingQuota,
];

pub struct DispatchCtx<'a> {
    pub jobs: JobCtx<'a>,
    pub clock: FleetSeconds,
    pub git: &'a dyn BaseTipSource,
}

pub type BaseTips = BTreeMap<(String, String), String>;

pub trait BaseTipSource {
    fn base_tip(&self, root: &str, base: &str) -> Result<String>;
}

impl BaseTipSource for Git {
    fn base_tip(&self, root: &str, base: &str) -> Result<String> {
        self.rev(Path::new(root), base)
    }
}

impl BaseTipSource for BaseTips {
    fn base_tip(&self, root: &str, base: &str) -> Result<String> {
        self.get(&(root.to_string(), base.to_string()))
            .cloned()
            .with_context(|| {
                format!(
                    "fleetd read no tip of the base branch {base} in {root} before the \
                     transaction: the bind waits for a pass that reads it"
                )
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundTree {
    pub worktree_id: String,
    pub path: String,
    pub branch: String,
    pub add_job: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    pub task_id: String,
    pub attempt_id: String,
    pub attempt: i64,
    pub worker_id: String,
    pub persona: String,
    pub worktree: Option<BoundTree>,
}

#[derive(Debug, Default)]
pub struct Dispatched {
    pub bound: Vec<Bound>,
    pub held: Vec<(String, Hold)>,
    pub failed: Vec<(String, String)>,
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

pub fn bind(
    tx: &Connection,
    ctx: &DispatchCtx<'_>,
    task_id: &str,
    persona: &str,
    worker: &NewWorker,
) -> Result<Bound> {
    if tx.is_autocommit() {
        bail!("the bind writes its rows in 1 open transaction of the ledger");
    }
    let task = tasks::get(tx, task_id)?;
    if task.status != TaskStatus::Ready.as_str() {
        bail!(
            "{} is {}, not ready: 1 task binds to 1 attempt (FM-14)",
            task.short(),
            task.status
        );
    }
    if let Some(why) = tasks::cancel_requested(tx, task_id)? {
        bail!("{} waits for its cancel: {why}", task.short());
    }
    if let Some(open) = open_attempt(tx, task_id)? {
        bail!(
            "{} has the attempt {open} that is not ended: 1 task has 1 live attempt (I1)",
            task.short()
        );
    }
    let n = task.attempts + 1;
    let attempt = attempt_id(task_id, n);
    let text = format!("dispatch binds {} to attempt {attempt}", task.short());
    let leased = TaskStatus::Leased.as_str();
    let guard = ctx.jobs.guard;
    transition(
        tx,
        guard,
        Entity::Task,
        task_id,
        leased,
        by_fleetd(&text),
        &NeedsYouList,
    )?;
    let worker_id = workers::insert(
        tx,
        &NewWorker {
            run_id: task.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: persona.to_string(),
            target_task_id: Some(task_id.to_string()),
            ..worker.clone()
        },
    )?;
    tx.execute(
        "UPDATE workers SET task_id = ?2, attempt = ?3 WHERE worker_id = ?1",
        params![worker_id, task_id, n],
    )?;
    tx.execute(
        &format!(
            "UPDATE tasks SET attempts = ?2, current_worker_id = ?3, updated_at = ?4
             WHERE task_id = ?1 AND status = {}",
            word(TaskStatus::Leased)
        ),
        params![task_id, n, worker_id, util::now()],
    )?;
    let row = AttemptRow {
        id: attempt.clone(),
        task_id: Some(task_id.to_string()),
        worker_id: worker_id.clone(),
        state: AttemptState::Launching,
        process: None,
        pane: None,
    };
    store::insert_attempt(tx, &row, &util::now())?;
    tx.execute("UPDATE attempts SET via = 'push' WHERE id = ?1", [&attempt])?;
    created(tx, guard, Entity::Attempt, &attempt, by_fleetd(&text))?;
    let worktree = if task.uses_worktree() {
        Some(bind_tree(tx, ctx, &task, &worker_id, &attempt)?)
    } else {
        None
    };
    if let Some(tree) = &worktree {
        landing::bind_branch(&ctx.jobs, tx, task_id, &tree.branch, ctx.clock)?;
    }
    Ok(Bound {
        task_id: task_id.to_string(),
        attempt_id: attempt,
        attempt: n,
        worker_id,
        persona: persona.to_string(),
        worktree,
    })
}

fn open_attempt(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM attempts WHERE task_id = ?1 AND state <> 'ended' ORDER BY rowid LIMIT 1",
    )?;
    let mut rows = stmt.query_map([task_id], |r| r.get(0))?;
    Ok(rows.next().transpose()?)
}

fn task_tree(conn: &Connection, task_id: &str) -> Result<Option<WorktreeRow>> {
    Ok(trees::of_task(conn, task_id)?.filter(|r| !WORKTREES_GONE.contains(&r.status)))
}

fn bind_tree(
    tx: &Connection,
    ctx: &DispatchCtx<'_>,
    task: &Task,
    worker_id: &str,
    attempt: &str,
) -> Result<BoundTree> {
    let new = match task_tree(tx, &task.task_id)? {
        Some(_) => None,
        None => Some(new_tree(tx, ctx.git, task, worker_id)?),
    };
    let lease = Lease {
        task_id: Some(&task.task_id),
        worker_id,
        attempt: Some(attempt),
        from: None,
        new: new.as_ref(),
    };
    let (row, add_job) = match create_worktree(&ctx.jobs, tx, &lease, ctx.clock)? {
        CreateOutcome::Attached(row) => (row, None),
        CreateOutcome::Reserved { row, job } => (row, Some(job)),
        CreateOutcome::NeedsRescue(row) => bail!(
            "the worktree {} of {} is orphaned: its rescue comes first",
            row.worktree_id,
            task.short()
        ),
    };
    Ok(BoundTree {
        worktree_id: row.worktree_id,
        path: row.path,
        branch: row.branch,
        add_job,
    })
}

fn base_branch(run: &Run) -> Result<String> {
    run.run_branch
        .clone()
        .or(run.base_branch.clone())
        .context("the run has no run branch: create runs in a git repository")
}

fn tree_base(conn: &Connection, run: &Run, task: &Task) -> Result<String> {
    let Some(judged) = judged_task(&task.kind, task.parent_task_id.as_deref()) else {
        return base_branch(run);
    };
    match task_tree(conn, judged)? {
        Some(row) => Ok(row.branch),
        None => bail!(
            "{} judges {}, which has no worktree: a judge tree starts from the judged branch",
            task.short(),
            tasks::get(conn, judged)?.short()
        ),
    }
}

fn new_tree(
    conn: &Connection,
    tips: &dyn BaseTipSource,
    task: &Task,
    worker_id: &str,
) -> Result<NewWorktree> {
    let run = runs::get(conn, &task.run_id)?;
    let root = Path::new(&run.project_root);
    let base = tree_base(conn, &run, task)?;
    let name = format!("T{}-{}", task.seq, slug(&task.title));
    let branch = format!("{}-{name}", run.run_branch.as_deref().unwrap_or("fleet"));
    let base_commit = tips.base_tip(&run.project_root, &base)?;
    Ok(NewWorktree {
        worktree_id: util::new_id(),
        task_id: Some(task.task_id.clone()),
        worker_id: Some(worker_id.to_string()),
        repo_root: run.project_root.clone(),
        path: root
            .join(".worktrees")
            .join(format!("{}-{name}", run.slug))
            .to_string_lossy()
            .into_owned(),
        branch,
        base,
        base_commit: Some(base_commit),
        status: WorktreeStatus::Open,
        mode: None,
    })
}

pub fn read_base_tips(conn: &Connection, gw: &dyn IntegrationGitGateway) -> Result<BaseTips> {
    let rows = ReadyRows::read(conn)?;
    let mut bases = BTreeSet::new();
    for task in &rows.tasks {
        if !task.uses_worktree() || rows.trees.contains_key(&task.task_id) {
            continue;
        }
        let run = rows.run(&task.run_id)?;
        if let Some(base) = rows.tree_base(run, task) {
            bases.insert((run.project_root.clone(), base));
        }
    }
    let mut tips = BaseTips::new();
    for (root, base) in bases {
        let name = format!("refs/heads/{base}");
        if let Some(tip) = gw.tip(Path::new(&root), &name)? {
            tips.insert((root, base), tip);
        }
    }
    Ok(tips)
}

fn slug(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    s.trim_matches('-').chars().take(40).collect()
}

#[cfg(test)]
fn task_ids(conn: &Connection, states: &[TaskStatus]) -> Result<Vec<String>> {
    let words: Vec<String> = states.iter().map(|s| word(*s)).collect();
    let sql = format!(
        "SELECT task_id FROM tasks WHERE status IN ({}) ORDER BY run_id, seq",
        words.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

fn persona_of(t: &Task) -> String {
    t.claimable_by
        .clone()
        .or(t.primary_persona.clone())
        .unwrap_or_default()
}

struct LiveTree {
    orphaned: bool,
    branch: String,
}

struct ReadyRows {
    tasks: Vec<Task>,
    cancelled: BTreeSet<String>,
    parked: BTreeMap<String, String>,
    own: BTreeMap<String, Vec<String>>,
    open: BTreeMap<String, String>,
    trees: BTreeMap<String, LiveTree>,
    runs: BTreeMap<String, Run>,
}

impl ReadyRows {
    fn read(conn: &Connection) -> Result<ReadyRows> {
        Self::read_with(conn, || Ok(()))
    }

    fn read_with(conn: &Connection, between: impl FnOnce() -> Result<()>) -> Result<ReadyRows> {
        one_snapshot(conn, || Self::read_now(conn, between))
    }

    fn read_now(conn: &Connection, between: impl FnOnce() -> Result<()>) -> Result<ReadyRows> {
        let ready = word(TaskStatus::Ready);
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM tasks WHERE status = {ready} ORDER BY run_id, seq"
        ))?;
        let tasks = stmt
            .query_map([], Task::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        between()?;
        let cancelled = pairs(
            conn,
            &format!(
                "SELECT task_id, cancel_requested FROM tasks
                 WHERE status = {ready} AND cancel_requested IS NOT NULL"
            ),
            [],
        )?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
        let parked_ideas = ideas_store::ids_in_state(conn, PARKED)?;
        let parked = pairs(
            conn,
            &format!(
                "SELECT task_id, idea_id FROM tasks
                 WHERE status = {ready} AND idea_id IS NOT NULL"
            ),
            [],
        )?
        .into_iter()
        .filter(|(_, idea)| parked_ideas.contains(idea))
        .collect();
        let own = tasks::files_of_tasks_in(conn, &[TaskStatus::Ready], FileMode::Own)?;
        let mut open = BTreeMap::new();
        for (task_id, id) in pairs(
            conn,
            &format!(
                "SELECT a.task_id, a.id FROM attempts a JOIN tasks t ON t.task_id = a.task_id
                 WHERE t.status = {ready} AND a.state <> 'ended' ORDER BY a.rowid"
            ),
            [],
        )? {
            open.entry(task_id).or_insert(id);
        }
        let trees = live_trees(conn, &tasks)?;
        let runs = runs::of_ids(conn, tasks.iter().map(|t| t.run_id.as_str()))?;
        Ok(ReadyRows {
            tasks,
            cancelled,
            parked,
            own,
            open,
            trees,
            runs,
        })
    }

    fn run(&self, run_id: &str) -> Result<&Run> {
        self.runs
            .get(run_id)
            .with_context(|| format!("no run {run_id}"))
    }

    fn tree_base(&self, run: &Run, task: &Task) -> Option<String> {
        match judged_task(&task.kind, task.parent_task_id.as_deref()) {
            Some(judged) => self.trees.get(judged).map(|t| t.branch.clone()),
            None => run.run_branch.clone().or(run.base_branch.clone()),
        }
    }

    fn ready_task(&mut self, t: Task, target: Option<&str>) -> Result<ReadyTask> {
        Ok(ReadyTask {
            short: t.short(),
            project: self.run(&t.run_id)?.project_id.clone(),
            persona: persona_of(&t),
            target: target == Some(t.task_id.as_str()),
            shared_checkout: !t.uses_worktree(),
            review: t.is_review(),
            own: self.own.remove(&t.task_id).unwrap_or_default(),
            open_attempt: self.open.remove(&t.task_id),
            tree_orphaned: self.trees.get(&t.task_id).is_some_and(|r| r.orphaned),
            priority: t.priority,
            seq: t.seq,
            not_before: t.not_before,
            parallel: t.parallel,
            run_id: t.run_id,
            task_id: t.task_id,
        })
    }
}

fn pairs(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params, |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn live_trees(conn: &Connection, ready: &[Task]) -> Result<BTreeMap<String, LiveTree>> {
    let ids = ready
        .iter()
        .flat_map(|t| std::iter::once(t.task_id.as_str()).chain(t.parent_task_id.as_deref()));
    let mut out = BTreeMap::new();
    for tree in trees::newest_of_tasks(conn, ids)? {
        if WORKTREES_GONE.contains(&WorktreeStatus::parse(&tree.status)?) {
            continue;
        }
        let orphaned = WorktreeState::parse(&tree.state)? == WorktreeState::Orphaned;
        let branch = tree.branch;
        out.insert(tree.task_id, LiveTree { orphaned, branch });
    }
    Ok(out)
}

fn running_tasks(conn: &Connection) -> Result<Vec<RunningTask>> {
    one_snapshot(conn, || running_tasks_now(conn))
}

fn running_tasks_now(conn: &Connection) -> Result<Vec<RunningTask>> {
    let words: Vec<String> = HOLDING.iter().map(|s| word(*s)).collect();
    let holding = words.join(", ");
    let mut own = tasks::files_of_tasks_in(conn, &HOLDING, FileMode::Own)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT * FROM tasks WHERE status IN ({holding}) ORDER BY run_id, seq"
    ))?;
    let tasks = stmt
        .query_map([], Task::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tasks
        .into_iter()
        .map(|t| RunningTask {
            short: t.short(),
            shared_checkout: !t.uses_worktree(),
            review: t.is_review(),
            own: own.remove(&t.task_id).unwrap_or_default(),
            parallel: t.parallel,
            run_id: t.run_id,
        })
        .collect())
}

pub fn choices(
    conn: &Connection,
    holds: &Holds,
    sandboxes: &dyn CheckSandboxes,
    target: Option<&str>,
) -> Result<Vec<Choice>> {
    one_snapshot(conn, || choices_now(conn, holds, sandboxes, target))
}

fn choices_now(
    conn: &Connection,
    holds: &Holds,
    sandboxes: &dyn CheckSandboxes,
    target: Option<&str>,
) -> Result<Vec<Choice>> {
    let mut rows = ReadyRows::read(conn)?;
    let mut ready = Vec::new();
    let mut parked = Vec::new();
    for t in std::mem::take(&mut rows.tasks) {
        if rows.cancelled.contains(&t.task_id) {
            continue;
        }
        if let Some(idea) = rows.parked.get(&t.task_id) {
            let hold = Hold::IdeaParked { idea: idea.clone() };
            parked.push(Choice::Held {
                task_id: t.task_id,
                hold,
            });
            continue;
        }
        ready.push(rows.ready_task(t, target)?);
    }
    let running = running_tasks(conn)?;
    let mut holds = holds.clone();
    holds
        .unhealthy
        .extend(health::unhealthy_profiles(conn)?.into_keys());
    holds.personas.extend(health::not_proved_profiles(conn)?);
    for t in &ready {
        if !holds.personas.contains_key(&t.persona) && !sandboxes.supported(&t.persona) {
            holds
                .personas
                .insert(t.persona.clone(), NO_CHECK_SANDBOX.to_string());
        }
    }
    parked.extend(choose(&ready, &running, &holds));
    Ok(parked)
}

pub fn dispatch_due(
    tx: &Connection,
    ctx: &DispatchCtx<'_>,
    holds: &Holds,
    sandboxes: &dyn CheckSandboxes,
    budget: usize,
) -> Result<Dispatched> {
    if tx.is_autocommit() {
        bail!("the dispatch writes its rows in 1 open transaction of the ledger");
    }
    let mut out = Dispatched::default();
    for choice in choices(tx, holds, sandboxes, None)? {
        match choice {
            Choice::Held { task_id, hold } => out.held.push((task_id, hold)),
            Choice::Bind { .. } if out.bound.len() >= budget => {}
            Choice::Bind { task_id, persona } => {
                let worker = dispatched_worker(tx, &task_id)?;
                tx.execute_batch("SAVEPOINT dispatch_bind")?;
                match bind(tx, ctx, &task_id, &persona, &worker) {
                    Ok(b) => {
                        tx.execute_batch("RELEASE dispatch_bind")?;
                        out.bound.push(b);
                    }
                    Err(e) => {
                        tx.execute_batch("ROLLBACK TO dispatch_bind; RELEASE dispatch_bind")?;
                        out.failed.push((task_id, format!("{e:#}")));
                    }
                }
            }
        }
    }
    Ok(out)
}

fn dispatched_worker(conn: &Connection, task_id: &str) -> Result<NewWorker> {
    let task = tasks::get(conn, task_id)?;
    let run = runs::get(conn, &task.run_id)?;
    Ok(NewWorker {
        workspace_id: run.workspace_id,
        ..NewWorker::default()
    })
}

struct AttemptRows {
    id: String,
    task: Task,
    run: Run,
    persona: Option<String>,
    n: Option<i64>,
}

impl AttemptRows {
    fn read(conn: &Connection, attempt_id: &str) -> Result<AttemptRows> {
        let (task_id, worker_id): (Option<String>, String) = conn
            .query_row(
                "SELECT task_id, worker_id FROM attempts WHERE id = ?1",
                [attempt_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .with_context(|| format!("no attempt {attempt_id}"))?;
        let task_id = task_id.with_context(|| format!("the attempt {attempt_id} has no task"))?;
        let task = tasks::get(conn, &task_id)?;
        let run = runs::get(conn, &task.run_id)?;
        let worker = workers::get(conn, &worker_id)?;
        Ok(AttemptRows {
            id: attempt_id.to_string(),
            task,
            run,
            persona: worker.persona,
            n: worker.attempt,
        })
    }

    fn launching_push(conn: &Connection) -> Result<Vec<AttemptRows>> {
        one_snapshot(conn, || {
            const LAUNCHING: &str =
                "a.via = 'push' AND a.state = 'launching' AND a.task_id IS NOT NULL";
            let mut stmt = conn.prepare(&format!(
                "SELECT a.id, a.task_id, w.persona, w.attempt FROM attempts a
                 JOIN workers w ON w.worker_id = a.worker_id
                 WHERE {LAUNCHING} ORDER BY a.rowid"
            ))?;
            let heads = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut stmt = conn.prepare(&format!(
                "SELECT * FROM tasks WHERE task_id IN
                 (SELECT a.task_id FROM attempts a WHERE {LAUNCHING})"
            ))?;
            let tasks: BTreeMap<String, Task> = stmt
                .query_map([], Task::from_row)?
                .map(|t| t.map(|t| (t.task_id.clone(), t)))
                .collect::<rusqlite::Result<_>>()?;
            let runs = runs::of_ids(conn, tasks.values().map(|t| t.run_id.as_str()))?;
            heads
                .into_iter()
                .map(|(id, task_id, persona, n)| {
                    let task = tasks
                        .get(&task_id)
                        .with_context(|| format!("no task {task_id} in the task ledger"))?
                        .clone();
                    let run = runs
                        .get(&task.run_id)
                        .with_context(|| format!("no run {} in the task ledger", task.run_id))?
                        .clone();
                    Ok(AttemptRows {
                        id,
                        task,
                        run,
                        persona,
                        n,
                    })
                })
                .collect()
        })
    }

    fn attempt(&self) -> Result<String> {
        let n = self
            .n
            .with_context(|| format!("the worker of attempt {} has no attempt number", self.id))?;
        Ok(format!("A{n}"))
    }
}

struct FileRows {
    repairs: BTreeMap<String, String>,
    reviews: BTreeMap<String, String>,
    answered: BTreeMap<String, Vec<questions::Answered>>,
    trees: BTreeMap<String, trees::TaskTree>,
    handoffs: BTreeMap<String, String>,
    marks: BTreeMap<String, (Option<String>, Option<String>)>,
    features: BTreeMap<String, String>,
    covers: BTreeMap<String, Vec<String>>,
}

impl FileRows {
    fn read(conn: &Connection, fleet_root: &Path, attempts: &[AttemptRows]) -> Result<FileRows> {
        let task_ids: Vec<&str> = attempts.iter().map(|a| a.task.task_id.as_str()).collect();
        let ids = || task_ids.iter().copied();
        let marks: BTreeMap<String, (Option<String>, Option<String>)> = {
            let sql = format!(
                "SELECT task_id, idea_id, json_extract(spec_json, '$.kind') FROM tasks
                 WHERE task_id IN ({})",
                crate::taskledger::sql_words::marks(1, task_ids.len())
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(ids()), |r| {
                Ok((r.get(0)?, (r.get(1)?, r.get(2)?)))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let idea_ids = marks.values().filter_map(|(idea, _)| idea.as_deref());
        let features = ideas_store::features_of(conn, idea_ids)?;
        let mut trees = BTreeMap::new();
        for tree in trees::newest_of_tasks(conn, ids())? {
            if !WORKTREES_GONE.contains(&WorktreeStatus::parse(&tree.status)?) {
                trees.insert(tree.task_id.clone(), tree);
            }
        }
        Ok(FileRows {
            repairs: repair::start_sections(
                conn,
                fleet_root,
                attempts.iter().map(|a| a.id.as_str()),
            )?,
            reviews: review::start_sections(conn, ids())?,
            answered: questions::answered_of_tasks(conn, ids())?,
            trees,
            handoffs: outbox_ingest::handoffs_of(conn, ids())?,
            marks,
            features,
            covers: ideas_store::covers_of(conn, ids())?,
        })
    }

    fn facts(&self, rows: &AttemptRows) -> Result<TaskFacts> {
        let task = rows.task.clone();
        let run = &rows.run;
        let task_id = task.task_id.clone();
        let work = match self.repairs.get(&rows.id) {
            Some(section) => Some(section.clone()),
            None => self.reviews.get(&task_id).cloned(),
        };
        let answered: Vec<AnsweredQuestion> = self
            .answered
            .get(&task_id)
            .into_iter()
            .flatten()
            .map(|q| AnsweredQuestion {
                id: q.id.clone(),
                question: q.question.clone(),
                answer: q.answer.clone(),
            })
            .collect();
        let sections: Vec<String> = work.into_iter().chain(answers_section(&answered)).collect();
        let extra = (!sections.is_empty()).then(|| sections.join("\n"));
        let tree = self.trees.get(&task_id).filter(|_| task.uses_worktree());
        let (worktree, branch) = match tree {
            Some(row) => (row.path.clone(), row.branch.clone()),
            None => (run.project_root.clone(), base_branch(run)?),
        };
        let (idea, spec_kind) = self.marks.get(&task_id).cloned().unwrap_or_default();
        let feature = idea.as_ref().and_then(|i| self.features.get(i)).cloned();
        let kind = task.work_kind.or(spec_kind.filter(|kind| !kind.is_empty()));
        Ok(TaskFacts {
            task: format!("T{}", task.seq),
            handoff: self.handoffs.get(&task_id).cloned(),
            covers: self.covers.get(&task_id).cloned().unwrap_or_default(),
            task_id,
            run: run.slug.clone(),
            attempt: rows.attempt()?,
            profile: rows.persona.clone().unwrap_or_default(),
            worktree,
            branch,
            read_first: task.task_file.unwrap_or_default(),
            goal: task.title,
            check: task.land_check,
            kind,
            ticket_snapshot: task.ticket_snapshot.unwrap_or_default(),
            extra,
            idea,
            feature,
            verdict: None,
        })
    }
}

pub fn write_task_files(
    conn: &Connection,
    fleet_root: &Path,
    attempt_id: &str,
    secrets: &[Secret],
) -> Result<AttemptFiles> {
    write_task_files_in(conn, fleet_root, None, attempt_id, secrets)
}

pub fn write_task_files_in(
    conn: &Connection,
    fleet_root: &Path,
    state_root: Option<&Path>,
    attempt_id: &str,
    secrets: &[Secret],
) -> Result<AttemptFiles> {
    let rows = AttemptRows::read(conn, attempt_id)?;
    let facts = FileRows::read(conn, fleet_root, std::slice::from_ref(&rows))?.facts(&rows)?;
    write_with_facts(fleet_root, state_root, &rows.run, &facts, secrets)
}

fn write_with_facts(
    fleet_root: &Path,
    state_root: Option<&Path>,
    run: &Run,
    facts: &TaskFacts,
    secrets: &[Secret],
) -> Result<AttemptFiles> {
    let project_facts = match state_root {
        Some(state) => Some(crate::project_notes::read(
            state,
            Path::new(&run.project_root),
        )?),
        None => None,
    };
    Ok(write_attempt_files_with(
        fleet_root,
        facts,
        project_facts.as_deref(),
        secrets,
    )?)
}

pub fn write_missing_task_files(
    conn: &Connection,
    fleet_root: &Path,
    secrets: &[Secret],
) -> Result<Vec<(String, Result<AttemptFiles>)>> {
    write_missing_task_files_in(conn, fleet_root, None, secrets)
}

pub fn write_missing_task_files_in(
    conn: &Connection,
    fleet_root: &Path,
    state_root: Option<&Path>,
    secrets: &[Secret],
) -> Result<Vec<(String, Result<AttemptFiles>)>> {
    let (missing, file_rows) = one_snapshot(conn, || {
        let mut missing = Vec::new();
        for rows in AttemptRows::launching_push(conn)? {
            let dir = attempt_dir(fleet_root, &rows.task.task_id, &rows.attempt()?);
            if !dir.join("task.yaml").exists() {
                missing.push(rows);
            }
        }
        if missing.is_empty() {
            return Ok((missing, None));
        }
        let file_rows = FileRows::read(conn, fleet_root, &missing)?;
        Ok((missing, Some(file_rows)))
    })?;
    let Some(file_rows) = file_rows else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for rows in missing {
        let facts = file_rows.facts(&rows)?;
        let written = write_with_facts(fleet_root, state_root, &rows.run, &facts, secrets);
        out.push((rows.id, written));
    }
    Ok(out)
}

#[cfg(test)]
mod running_tasks_tests {
    use super::*;
    use crate::taskledger::store::Store;

    type Row<'a> = (
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        bool,
        &'a [&'a str],
        &'a [&'a str],
    );

    fn ledger(tasks: &[Row<'_>]) -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("ledger.sqlite3")).unwrap();
        store
            .immediate(|tx| {
                tx.execute_batch(
                    "INSERT INTO projects (project_id, root, created_at) VALUES ('p1', '/p', 'now');
                     INSERT INTO runs (run_id, project_id, slug, finish_steps, status, started_at,
                         max_attempts)
                     VALUES ('r1', 'p1', 'r1', '[]', 'active', 'now', 5);",
                )?;
                let idea = crate::taskledger::schema::run_idea(tx, "r1", "now", true)?;
                for (id, status, kind, mode, parallel, own, dnt) in tasks {
                    tx.execute(
                        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona,
                             claimable_by, status, kind, worktree_mode, parallel, created_at, updated_at, idea_id)
                         VALUES (?1, 'r1', (SELECT count(*) + 1 FROM tasks), ?1, 'a.md', 'opus',
                             'opus', ?2, ?3, ?4, ?5, 'now', 'now', ?6)",
                        params![id, status, kind, mode, parallel, idea],
                    )?;
                    for (paths, m) in [(own, "own"), (dnt, "do_not_touch")] {
                        for p in paths.iter() {
                            tx.execute(
                                "INSERT INTO task_files (task_id, path, mode) VALUES (?1, ?2, ?3)",
                                params![id, p, m],
                            )?;
                        }
                    }
                }
                Ok(())
            })
            .unwrap();
        (tmp, store)
    }

    fn running_one_by_one(conn: &Connection) -> Vec<RunningTask> {
        task_ids(conn, &HOLDING)
            .unwrap()
            .into_iter()
            .map(|id| {
                let t = tasks::get(conn, &id).unwrap();
                RunningTask {
                    short: t.short(),
                    shared_checkout: !t.uses_worktree(),
                    review: t.is_review(),
                    own: tasks::files(conn, &t.task_id, FileMode::Own).unwrap(),
                    parallel: t.parallel,
                    run_id: t.run_id,
                }
            })
            .collect()
    }

    #[test]
    fn running_tasks_gives_the_rows_of_the_read_of_each_task() {
        let (_tmp, store) = ledger(&[
            (
                "a",
                "running",
                "work",
                "worktree",
                true,
                &["src/b.rs", "src/a.rs"],
                &["x"],
            ),
            ("b", "ready", "work", "worktree", true, &["src/a.rs"], &[]),
            ("c", "leased", "review", "shared", false, &[], &["src/a.rs"]),
            ("d", "verifying", "work", "worktree", false, &["d.rs"], &[]),
            ("e", "done", "work", "worktree", true, &["e.rs"], &[]),
            (
                "f",
                "stalled",
                "work",
                "shared",
                true,
                &["f/1", "f/2", "f/0"],
                &[],
            ),
            ("g", "blocked_on_user", "work", "worktree", true, &[], &[]),
            (
                "h",
                "waiting_quota",
                "review",
                "worktree",
                true,
                &["h.rs"],
                &["h.rs"],
            ),
        ]);
        let set = running_tasks(store.conn()).unwrap();
        assert_eq!(set, running_one_by_one(store.conn()));
        let shorts: Vec<_> = set.iter().map(|t| t.short.as_str()).collect();
        assert_eq!(shorts, ["T-1", "T-3", "T-4", "T-6", "T-7", "T-8"]);
        assert_eq!(set[0].own, ["src/a.rs", "src/b.rs"]);
    }

    #[test]
    fn running_tasks_runs_the_same_statements_for_3_and_300_tasks() {
        let count = |n: usize| {
            let ids: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
            let rows: Vec<Row<'_>> = ids
                .iter()
                .map(|id| {
                    (
                        id.as_str(),
                        "running",
                        "work",
                        "worktree",
                        true,
                        &["a.rs"][..],
                        &[][..],
                    )
                })
                .collect();
            let (_tmp, store) = ledger(&rows);
            let (read, statements) =
                crate::taskledger::testing::count_statements(store.conn(), || {
                    running_tasks(store.conn()).unwrap()
                });
            assert_eq!(read.len(), n);
            statements
        };
        assert_eq!(count(3), count(300));
    }
}

#[cfg(test)]
mod ready_rows_race_tests {
    use super::*;
    use crate::taskledger::store::Store;

    fn add_task(tx: &Connection, id: &str, run: &str, own: &str) -> Result<()> {
        let idea = crate::taskledger::schema::run_idea(tx, run, "now", true)?;
        tx.execute(
            "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona,
                 claimable_by, status, created_at, updated_at, idea_id)
             VALUES (?1, ?2, (SELECT count(*) + 1 FROM tasks), ?1, 'a.md', 'opus', 'opus',
                 'ready', 'now', 'now', ?3)",
            params![id, run, idea],
        )?;
        tx.execute(
            "INSERT INTO task_files (task_id, path, mode) VALUES (?1, ?2, 'own')",
            params![id, own],
        )?;
        Ok(())
    }

    #[test]
    fn ready_rows_read_one_moment_while_another_connection_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ledger.sqlite3");
        let mut store = Store::open(&path).unwrap();
        store
            .immediate(|tx| {
                tx.execute_batch(
                    "INSERT INTO projects (project_id, root, created_at) VALUES ('p1', '/p', 'now');
                     INSERT INTO runs (run_id, project_id, slug, finish_steps, status, started_at)
                     VALUES ('r1', 'p1', 'r1', '[]', 'active', 'now'),
                            ('r2', 'p1', 'r2', '[]', 'active', 'now');",
                )?;
                add_task(tx, "a", "r1", "a.rs")
            })
            .unwrap();
        let mut other = Store::open(&path).unwrap();
        let rows = ReadyRows::read_with(store.conn(), || {
            other.immediate(|tx| {
                tx.execute("UPDATE tasks SET status = 'leased' WHERE task_id = 'a'", [])?;
                add_task(tx, "n", "r2", "n.rs")
            })
        })
        .unwrap();
        for t in &rows.tasks {
            rows.run(&t.run_id).unwrap();
        }
        let in_tasks = rows.tasks.iter().any(|t| t.task_id == "n");
        assert_eq!(in_tasks, rows.own.contains_key("n"));
        let a_in_tasks = rows.tasks.iter().any(|t| t.task_id == "a");
        assert_eq!(a_in_tasks, rows.own.contains_key("a"));
    }
}

#[cfg(test)]
mod missing_task_files_tests {
    use super::*;
    use crate::fleetd::task_execution::adapters::verdict_file;
    use crate::fleetd::task_execution::domain::verification::{Decision, Verdict};
    use crate::taskledger::store::Store;
    use crate::taskledger::testing::count_statements;

    fn ledger(n: usize) -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("ledger.sqlite3")).unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        store
            .immediate(|tx| {
                tx.execute(
                    "INSERT INTO projects (project_id, root, created_at) VALUES ('p1', ?1, 'now')",
                    [root.to_str().unwrap()],
                )?;
                tx.execute_batch(
                    "INSERT INTO runs (run_id, project_id, slug, finish_steps, status, started_at,
                         run_branch)
                     VALUES ('r1', 'p1', 'r1', '[]', 'active', 'now', 'fleet/r1');",
                )?;
                let idea = crate::taskledger::schema::run_idea(tx, "r1", "now", true)?;
                for i in 0..n {
                    let id = format!("t{i}");
                    tx.execute(
                        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona,
                             claimable_by, status, worktree_mode, created_at, updated_at, idea_id)
                         VALUES (?1, 'r1', ?2, ?1, 'a.md', 'opus', 'opus', 'leased', 'shared',
                             'now', 'now', ?3)",
                        params![id, i + 1, idea],
                    )?;
                    tx.execute(
                        "INSERT INTO workers (worker_id, run_id, persona, host, status, task_id,
                             attempt)
                         VALUES ('w' || ?1, 'r1', 'opus', 'h', 'spawning', ?1, 1)",
                        [&id],
                    )?;
                    tx.execute(
                        "INSERT INTO attempts (id, task_id, worker_id, state, via)
                         VALUES ('a' || ?1, ?1, 'w' || ?1, 'launching', 'push')",
                        [&id],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        (tmp, store)
    }

    fn statements(n: usize, state: bool) -> usize {
        let (tmp, store) = ledger(n);
        let fleet_root = tmp.path().join("fleet");
        let state_root = tmp.path().join("state");
        let state_root = state.then_some(state_root.as_path());
        let (written, count) = count_statements(store.conn(), || {
            write_missing_task_files_in(store.conn(), &fleet_root, state_root, &[]).unwrap()
        });
        assert_eq!(written.len(), n);
        for (id, files) in &written {
            assert!(files.as_ref().unwrap().task_yaml.is_file(), "{id}");
        }
        let (again, count_again) = count_statements(store.conn(), || {
            write_missing_task_files_in(store.conn(), &fleet_root, state_root, &[]).unwrap()
        });
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(count_again, ATTEMPT_ROWS_READS, "attempts with files: {n}");
        count
    }

    const ATTEMPT_ROWS_READS: usize = 4;

    fn add_task_rows(store: &mut Store) {
        store
            .immediate(|tx| {
                tx.execute_batch(
                    "INSERT INTO questions (id, task_id, feature_id, owner, state, due_at,
                         asked_at, answered_at)
                     VALUES ('q1', 't0', 'F-1', 'o', 'answered', 0, 'now', 'now');
                     INSERT INTO events (at, actor, kind, text, entity, entity_id, task_id,
                         data_json)
                     VALUES ('now', 'horch', 'question', 'q', 'task', 't0', 't0',
                             '{\"question_id\": \"q1\", \"text\": \"why\"}'),
                            ('now', 'horch', 'answered', 'a', 'task', 't0', 't0',
                             '{\"question_id\": \"q1\", \"text\": \"because\"}');
                     INSERT INTO task_covers (task_id, req_id) VALUES ('t0', 'R1'), ('t0', 'R2');
                     INSERT INTO workers (worker_id, run_id, persona, host, status, task_id,
                         attempt, handoff_path)
                     VALUES ('wold', 'r1', 'opus', 'h', 'stopped', 't0', 0, '/h.md');
                     INSERT INTO attempts (id, task_id, worker_id, state, outcome)
                     VALUES ('aold', 't0', 'wold', 'ended', 'handed_off');",
                )?;
                verdict_file::record(
                    tx,
                    None,
                    Verdict {
                        task_id: "t1".into(),
                        attempt: "A1".into(),
                        n: 0,
                        decision: Decision::Failed,
                        tip: "abc".into(),
                        check_hash: "h".into(),
                        job: None,
                        reason: "code-sweep: F1".into(),
                    },
                )?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn file_rows_give_the_rows_of_the_read_of_each_task() {
        let (tmp, mut store) = ledger(3);
        add_task_rows(&mut store);
        let conn = store.conn();
        let fleet_root = tmp.path().join("fleet");
        let attempts = AttemptRows::launching_push(conn).unwrap();
        let file_rows = FileRows::read(conn, &fleet_root, &attempts).unwrap();
        for rows in &attempts {
            let facts = file_rows.facts(rows).unwrap();
            let task_id = rows.task.task_id.as_str();
            let answered: Vec<AnsweredQuestion> = questions::answered_of_task(conn, task_id)
                .unwrap()
                .into_iter()
                .map(|q| AnsweredQuestion {
                    id: q.id,
                    question: q.question,
                    answer: q.answer,
                })
                .collect();
            let review = review::start_section(conn, task_id).unwrap();
            let sections: Vec<String> = review
                .into_iter()
                .chain(answers_section(&answered))
                .collect();
            let extra = (!sections.is_empty()).then(|| sections.join("\n"));
            let idea = ideas_store::idea(conn, facts.idea.as_deref().unwrap()).unwrap();
            assert_eq!(facts.extra, extra, "{task_id}");
            assert_eq!(
                facts.handoff,
                outbox_ingest::handoff_of(conn, task_id).unwrap()
            );
            assert_eq!(facts.covers, ideas_store::covers(conn, task_id).unwrap());
            assert_eq!(facts.feature, idea.map(|row| row.feature_id));
        }
        let t0 = file_rows.facts(&attempts[0]).unwrap();
        assert!(t0.extra.unwrap().contains("because"));
        assert_eq!(t0.handoff.as_deref(), Some("/h.md"));
        assert_eq!(t0.covers, ["R1", "R2"]);
        let t1 = file_rows.facts(&attempts[1]).unwrap();
        assert!(t1.extra.unwrap().contains("code-sweep: F1"));
        assert_eq!(file_rows.facts(&attempts[2]).unwrap().extra, None);
    }

    #[test]
    fn missing_task_files_of_3_and_300_attempts_run_the_same_statements() {
        assert_eq!(statements(3, true), statements(300, true));
    }

    #[test]
    fn project_facts_of_3_and_300_missing_attempts_run_no_statement() {
        for n in [3, 300] {
            assert_eq!(statements(n, true), statements(n, false), "{n} attempts");
        }
    }
}
