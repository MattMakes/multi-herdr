//! Tasks: queueing, the claimable rule and the claim (sections 4.3 to 4.5,
//! 5.1, 5.2).

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Row};
use serde::Serialize;

use super::model::{
    Actor, EventKind, FileMode, Origin, TaskKind, TaskStatus, WorkerKind, WorkerStatus,
    WorktreeMode,
};
use super::runs::Run;
use super::store::optional;
use super::util::{self, About};
use super::workers::{self, Worker};

/// How much a retried task moves ahead of the queue (section 5.4).
pub const RETRY_PRIORITY_BOOST: i64 = 1000;

/// 1 task row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Task {
    pub task_id: String,
    pub run_id: String,
    pub seq: i64,
    pub title: String,
    pub task_file: Option<String>,
    pub issue_url: Option<String>,
    pub spec_ref: Option<String>,
    pub primary_persona: Option<String>,
    pub fallback_persona: Option<String>,
    pub claimable_by: Option<String>,
    pub fallback_reason: Option<String>,
    pub phase: Option<String>,
    pub parallel: bool,
    pub priority: i64,
    pub wave: Option<i64>,
    pub worktree_mode: String,
    pub land_check: Option<String>,
    pub kind: String,
    pub parent_task_id: Option<String>,
    pub status: String,
    pub attempts: i64,
    /// Set while a `queued` task is held for the operator after too many
    /// attempts (#168).
    pub held_reason: Option<String>,
    /// `attempts` at the last operator requeue.
    pub attempt_floor: i64,
    pub current_worker_id: Option<String>,
    pub summary: Option<String>,
    pub origin: String,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
}

impl Task {
    /// `T-<seq>`: the short name people use.
    pub fn short(&self) -> String {
        format!("T-{}", self.seq)
    }

    pub fn status(&self) -> Result<TaskStatus> {
        TaskStatus::parse(&self.status)
    }

    pub fn is_review(&self) -> bool {
        self.kind == TaskKind::Review.as_str()
    }

    pub fn uses_worktree(&self) -> bool {
        self.worktree_mode == WorktreeMode::Worktree.as_str()
    }

    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Task> {
        Ok(Task {
            task_id: r.get("task_id")?,
            run_id: r.get("run_id")?,
            seq: r.get("seq")?,
            title: r.get("title")?,
            task_file: r.get("task_file")?,
            issue_url: r.get("issue_url")?,
            spec_ref: r.get("spec_ref")?,
            primary_persona: r.get("primary_persona")?,
            fallback_persona: r.get("fallback_persona")?,
            claimable_by: r.get("claimable_by")?,
            fallback_reason: r.get("fallback_reason")?,
            phase: r.get("phase")?,
            parallel: r.get::<_, i64>("parallel")? != 0,
            priority: r.get("priority")?,
            wave: r.get("wave")?,
            worktree_mode: r.get("worktree_mode")?,
            land_check: r.get("land_check")?,
            kind: r.get("kind")?,
            parent_task_id: r.get("parent_task_id")?,
            status: r.get("status")?,
            attempts: r.get("attempts")?,
            held_reason: r.get("held_reason")?,
            attempt_floor: r.get("attempt_floor")?,
            current_worker_id: r.get("current_worker_id")?,
            summary: r.get("summary")?,
            origin: r.get("origin")?,
            created_at: r.get("created_at")?,
            updated_at: r.get("updated_at")?,
            finished_at: r.get("finished_at")?,
        })
    }
}

/// What `horch ledger add` queues.
#[derive(Debug, Clone)]
pub struct NewTask {
    pub title: String,
    pub task_file: String,
    pub issue_url: Option<String>,
    pub spec_ref: Option<String>,
    pub primary: String,
    pub fallback: Option<String>,
    pub phase: Option<String>,
    pub parallel: bool,
    pub priority: i64,
    pub wave: Option<i64>,
    pub worktree_mode: WorktreeMode,
    pub land_check: Option<String>,
    pub kind: TaskKind,
    pub parent_task_id: Option<String>,
    /// Task ids or `T-<n>` names in the same run.
    pub blocked_by: Vec<String>,
    pub own: Vec<String>,
    pub no_touch: Vec<String>,
}

impl NewTask {
    /// A plain work task with the defaults.
    pub fn work(title: &str, task_file: &str, primary: &str) -> NewTask {
        NewTask {
            title: title.to_string(),
            task_file: task_file.to_string(),
            issue_url: None,
            spec_ref: None,
            primary: primary.to_string(),
            fallback: None,
            phase: None,
            parallel: true,
            priority: 100,
            wave: None,
            worktree_mode: WorktreeMode::Worktree,
            land_check: None,
            kind: TaskKind::Work,
            parent_task_id: None,
            blocked_by: Vec::new(),
            own: Vec::new(),
            no_touch: Vec::new(),
        }
    }
}

/// 1 task by id.
pub fn get(conn: &Connection, task_id: &str) -> Result<Task> {
    optional(conn.query_row(
        "SELECT * FROM tasks WHERE task_id = ?1",
        [task_id],
        Task::from_row,
    ))?
    .with_context(|| format!("no task {task_id}"))
}

/// A task of `run_id` by id or `T-<n>`.
pub fn resolve(conn: &Connection, run_id: &str, key: &str) -> Result<Task> {
    let key = key.trim();
    let seq = key
        .strip_prefix("T-")
        .or_else(|| key.strip_prefix("t-"))
        .and_then(|n| n.parse::<i64>().ok());
    let found = match seq {
        Some(seq) => optional(conn.query_row(
            "SELECT * FROM tasks WHERE run_id = ?1 AND seq = ?2",
            params![run_id, seq],
            Task::from_row,
        ))?,
        None => optional(conn.query_row(
            "SELECT * FROM tasks WHERE run_id = ?1 AND task_id = ?2",
            params![run_id, key],
            Task::from_row,
        ))?,
    };
    found.with_context(|| format!("no task '{key}' in this run"))
}

/// Every task of a run, in `seq` order.
pub fn of_run(conn: &Connection, run_id: &str) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare("SELECT * FROM tasks WHERE run_id = ?1 ORDER BY seq")?;
    let rows = stmt
        .query_map([run_id], Task::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The tasks `task_id` waits for.
pub fn blockers(conn: &Connection, task_id: &str) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(
        "SELECT t.* FROM task_deps d JOIN tasks t ON t.task_id = d.blocked_by
         WHERE d.task_id = ?1 ORDER BY t.seq",
    )?;
    let rows = stmt
        .query_map([task_id], Task::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The files a task owns or must not touch.
pub fn files(conn: &Connection, task_id: &str, mode: FileMode) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT path FROM task_files WHERE task_id = ?1 AND mode = ?2 ORDER BY path")?;
    let rows = stmt
        .query_map(params![task_id, mode.as_str()], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(rows)
}

/// Queue 1 task. Blockers must already exist in the run, so a single add
/// cannot close a cycle; [`add_many`] checks cycles among new tasks.
pub fn add(conn: &Connection, run: &Run, new: &NewTask, actor: Actor) -> Result<Task> {
    validate(new)?;
    let blockers: Vec<Task> = new
        .blocked_by
        .iter()
        .map(|b| resolve(conn, &run.run_id, b))
        .collect::<Result<_>>()?;
    let id = insert(conn, run, new)?;
    for b in &blockers {
        conn.execute(
            "INSERT INTO task_deps (task_id, blocked_by) VALUES (?1, ?2)",
            params![id, b.task_id],
        )?;
    }
    let task = get(conn, &id)?;
    util::event(
        conn,
        &About {
            run: Some(&run.run_id),
            task: Some(&id),
            worker: None,
        },
        actor,
        EventKind::Added,
        &format!(
            "{} queued for {}: {}.",
            task.short(),
            new.primary,
            task.title
        ),
    )?;
    Ok(task)
}

/// Queue several tasks whose blockers may name each other by index. Any
/// invalid task, unknown blocker or cycle queues nothing (the caller runs
/// this in 1 transaction).
pub fn add_many(
    conn: &Connection,
    run: &Run,
    items: &[(String, NewTask)],
    actor: Actor,
) -> Result<Vec<Task>> {
    let keys: BTreeMap<&str, usize> = items
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (k.as_str(), i))
        .collect();
    if keys.len() != items.len() {
        bail!("2 tickets in the map share a number");
    }
    // Edges inside the map; blockers outside it must already exist.
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    for (i, (_, new)) in items.iter().enumerate() {
        validate(new)?;
        for b in &new.blocked_by {
            match keys.get(b.as_str()) {
                Some(&j) => edges[i].push(j),
                None => {
                    resolve(conn, &run.run_id, b).with_context(|| {
                        format!("ticket {} is blocked by unknown '{b}'", items[i].0)
                    })?;
                }
            }
        }
    }
    if let Some(cycle) = find_cycle(&edges) {
        let names: Vec<&str> = cycle.iter().map(|&i| items[i].0.as_str()).collect();
        bail!("the map has a dependency cycle: {}", names.join(" -> "));
    }
    // Insert in dependency order so every edge points at an existing row.
    let order = topo_order(&edges);
    let mut ids: Vec<Option<String>> = vec![None; items.len()];
    for &i in &order {
        let (_, new) = &items[i];
        let id = insert(conn, run, new)?;
        for b in &new.blocked_by {
            let blocker = match keys.get(b.as_str()) {
                Some(&j) => ids[j].clone().expect("blockers insert first"),
                None => resolve(conn, &run.run_id, b)?.task_id,
            };
            conn.execute(
                "INSERT INTO task_deps (task_id, blocked_by) VALUES (?1, ?2)",
                params![id, blocker],
            )?;
        }
        ids[i] = Some(id);
    }
    let mut out = Vec::new();
    for (i, id) in ids.into_iter().enumerate() {
        let task = get(conn, &id.expect("every item inserted"))?;
        util::event(
            conn,
            &About {
                run: Some(&run.run_id),
                task: Some(&task.task_id),
                worker: None,
            },
            actor,
            EventKind::Added,
            &format!(
                "{} queued from ticket {} for {}: {}.",
                task.short(),
                items[i].0,
                items[i].1.primary,
                task.title
            ),
        )?;
        out.push(task);
    }
    out.sort_by_key(|t| t.seq);
    Ok(out)
}

fn validate(new: &NewTask) -> Result<()> {
    if new.title.trim().is_empty() {
        bail!("a task needs a title");
    }
    if new.task_file.trim().is_empty() {
        bail!("a task needs a ticket file");
    }
    if new.primary.trim().is_empty() {
        bail!("a task needs a primary persona");
    }
    if new.fallback.as_deref() == Some(new.primary.as_str()) {
        bail!("the fallback persona is the primary persona");
    }
    Ok(())
}

fn insert(conn: &Connection, run: &Run, new: &NewTask) -> Result<String> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM tasks WHERE run_id = ?1",
        [&run.run_id],
        |r| r.get(0),
    )?;
    let id = util::new_id();
    let now = util::now();
    conn.execute(
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, issue_url, spec_ref,
            primary_persona, fallback_persona, claimable_by, phase, parallel, priority, wave,
            worktree_mode, land_check, kind, parent_task_id, status, origin, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20, ?20)",
        params![
            id,
            run.run_id,
            seq,
            new.title,
            new.task_file,
            new.issue_url,
            new.spec_ref,
            new.primary,
            new.fallback,
            new.phase,
            new.parallel as i64,
            new.priority,
            new.wave,
            new.worktree_mode.as_str(),
            new.land_check,
            new.kind.as_str(),
            new.parent_task_id,
            TaskStatus::Queued.as_str(),
            Origin::Native.as_str(),
            now,
        ],
    )?;
    for (paths, mode) in [
        (&new.own, FileMode::Own),
        (&new.no_touch, FileMode::DoNotTouch),
    ] {
        for p in paths {
            conn.execute(
                "INSERT OR IGNORE INTO task_files (task_id, path, mode) VALUES (?1, ?2, ?3)",
                params![id, p.trim(), mode.as_str()],
            )?;
        }
    }
    Ok(id)
}

/// A cycle in `edges` (node -> the nodes it waits for), if any.
fn find_cycle(edges: &[Vec<usize>]) -> Option<Vec<usize>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Open,
        Closed,
    }
    fn visit(n: usize, edges: &[Vec<usize>], mark: &mut [Mark], path: &mut Vec<usize>) -> bool {
        mark[n] = Mark::Open;
        path.push(n);
        for &m in &edges[n] {
            match mark[m] {
                Mark::Open => {
                    let at = path.iter().position(|&p| p == m).unwrap_or(0);
                    path.drain(..at);
                    path.push(m);
                    return true;
                }
                Mark::New => {
                    if visit(m, edges, mark, path) {
                        return true;
                    }
                }
                Mark::Closed => {}
            }
        }
        path.pop();
        mark[n] = Mark::Closed;
        false
    }
    let mut mark = vec![Mark::New; edges.len()];
    for n in 0..edges.len() {
        if mark[n] == Mark::New {
            let mut path = Vec::new();
            if visit(n, edges, &mut mark, &mut path) {
                return Some(path);
            }
        }
    }
    None
}

/// Nodes with every blocker first. `edges` has no cycle.
fn topo_order(edges: &[Vec<usize>]) -> Vec<usize> {
    fn visit(n: usize, edges: &[Vec<usize>], done: &mut [bool], out: &mut Vec<usize>) {
        if done[n] {
            return;
        }
        done[n] = true;
        for &m in &edges[n] {
            visit(m, edges, done, out);
        }
        out.push(n);
    }
    let mut done = vec![false; edges.len()];
    let mut out = Vec::new();
    for n in 0..edges.len() {
        visit(n, edges, &mut done, &mut out);
    }
    out
}

/// Why a queued task cannot be claimed now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Wait {
    /// A blocker is not done.
    BlockedBy { task: String, status: String },
    /// An exclusive task runs, or this exclusive task waits for others.
    Exclusive { running: String },
    /// A running task owns 1 of this task's files.
    FileHeld { path: String, by: String },
}

impl std::fmt::Display for Wait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Wait::BlockedBy { task, status } => write!(f, "blocked by {task} ({status})"),
            Wait::Exclusive { running } => write!(f, "exclusive: waits for {running}"),
            Wait::FileHeld { path, by } => write!(f, "{path} is held by {by}"),
        }
    }
}

/// The state the claimable rule reads: the run's tasks with their files and
/// blockers.
struct Board {
    tasks: Vec<Task>,
    own: BTreeMap<String, Vec<String>>,
    deps: BTreeMap<String, Vec<String>>,
}

impl Board {
    fn load(conn: &Connection, run_id: &str) -> Result<Board> {
        let tasks = of_run(conn, run_id)?;
        let mut own: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT f.task_id, f.path FROM task_files f JOIN tasks t ON t.task_id = f.task_id
                 WHERE t.run_id = ?1 AND f.mode = 'own'",
            )?;
            for row in stmt.query_map([run_id], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))? {
                let (t, p) = row?;
                own.entry(t).or_default().push(p);
            }
        }
        {
            let mut stmt = conn.prepare(
                "SELECT d.task_id, d.blocked_by FROM task_deps d JOIN tasks t ON t.task_id = d.task_id
                 WHERE t.run_id = ?1",
            )?;
            for row in stmt.query_map([run_id], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))? {
                let (t, b) = row?;
                deps.entry(t).or_default().push(b);
            }
        }
        Ok(Board { tasks, own, deps })
    }

    fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.task_id == id)
    }

    fn is_active(t: &Task) -> bool {
        matches!(t.status.as_str(), "running" | "landing")
    }

    /// Why `t` cannot start beside the `active` tasks, or `None`.
    fn wait(&self, t: &Task, active: &[&Task]) -> Option<Wait> {
        for b in self.deps.get(&t.task_id).into_iter().flatten() {
            if let Some(bt) = self.task(b) {
                if bt.status != TaskStatus::Done.as_str() {
                    return Some(Wait::BlockedBy {
                        task: bt.short(),
                        status: bt.status.clone(),
                    });
                }
            }
        }
        // Review tasks change no code: they never conflict on files and
        // never wait for an exclusive task (that task waits for them).
        if t.is_review() {
            return None;
        }
        let work: Vec<&&Task> = active.iter().filter(|a| !a.is_review()).collect();
        if let Some(x) = work.iter().find(|a| !a.parallel) {
            return Some(Wait::Exclusive { running: x.short() });
        }
        if !t.parallel {
            if let Some(a) = work.first() {
                return Some(Wait::Exclusive { running: a.short() });
            }
        }
        let mine = self.own.get(&t.task_id).cloned().unwrap_or_default();
        for a in &work {
            for theirs in self.own.get(&a.task_id).into_iter().flatten() {
                if let Some(p) = mine.iter().find(|m| util::paths_overlap(m, theirs)) {
                    return Some(Wait::FileHeld {
                        path: p.clone(),
                        by: a.short(),
                    });
                }
            }
        }
        None
    }

    /// Queued tasks in claim order: priority, then seq.
    fn queued(&self) -> Vec<&Task> {
        let mut q: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Queued.as_str())
            .collect();
        q.sort_by_key(|t| (t.priority, t.seq));
        q
    }

    /// Every queued task in claim order, with why it cannot start (`None`
    /// when it can). The tasks are chosen greedily: each one is checked
    /// against the running tasks and the startable ones before it.
    fn plan(&self) -> Vec<(&Task, Option<Wait>)> {
        let mut active: Vec<&Task> = self.tasks.iter().filter(|t| Self::is_active(t)).collect();
        let mut out = Vec::new();
        for t in self.queued() {
            let w = self.wait(t, &active);
            if w.is_none() {
                active.push(t);
            }
            out.push((t, w));
        }
        out
    }

    /// The queued tasks that can all start now.
    fn startable(&self) -> Vec<&Task> {
        self.plan()
            .into_iter()
            .filter(|(_, w)| w.is_none())
            .map(|(t, _)| t)
            .collect()
    }
}

/// The queued tasks that can start now, all together.
pub fn startable(conn: &Connection, run_id: &str) -> Result<Vec<Task>> {
    let board = Board::load(conn, run_id)?;
    Ok(board.startable().into_iter().cloned().collect())
}

/// Why each queued task that cannot start now waits.
pub fn waits(conn: &Connection, run_id: &str) -> Result<Vec<(Task, Wait)>> {
    let board = Board::load(conn, run_id)?;
    Ok(board
        .plan()
        .into_iter()
        .filter_map(|(t, w)| w.map(|w| (t.clone(), w)))
        .collect())
}

/// 1 earlier attempt, as a claim hands it to the next worker.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriorAttempt {
    pub worker_id: String,
    pub role: Option<String>,
    pub attempt: Option<i64>,
    pub status: String,
    pub end_reason: Option<String>,
    pub summary: Option<String>,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
}

/// What a successful claim gives the worker.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Claimed {
    pub task: Task,
    pub own: Vec<String>,
    pub do_not_touch: Vec<String>,
    pub attempt: i64,
    pub prior: Vec<PriorAttempt>,
    /// The open worktree from an earlier attempt, now this worker's.
    pub worktree: Option<String>,
    /// The unfinished past work this task was reopened from.
    pub reopened_from: Option<String>,
    pub worktree_branch: Option<String>,
}

/// The result of `horch ledger claim`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ClaimOutcome {
    Claimed(Box<Claimed>),
    NoTask {
        persona: String,
        role: Option<String>,
        supervisor_pid: Option<i64>,
        /// The exact line the worker sends the orchestrator.
        tell: String,
    },
}

/// Claim the next task for `worker_id`. Run it in an immediate
/// transaction: the read of the board and the write of the claim must not
/// interleave with another claim.
pub fn claim(conn: &Connection, worker_id: &str) -> Result<ClaimOutcome> {
    let w = workers::get(conn, worker_id)?;
    if w.kind == WorkerKind::Orchestrator.as_str() {
        bail!("the orchestrator does not claim tasks");
    }
    let status = w.status()?;
    let run_id = w.run_id.clone().context("this worker has no run")?;
    let persona = w.persona.clone().context("this worker has no persona")?;
    // A second claim by a working worker gives back its own task.
    if status == WorkerStatus::Working {
        if let Some(t) = &w.task_id {
            let task = get(conn, t)?;
            return Ok(ClaimOutcome::Claimed(Box::new(claimed(conn, &w, task)?)));
        }
    }
    if !status.is_live() {
        bail!(
            "worker {} is {}; a worker that ended cannot claim",
            w.label(),
            w.status
        );
    }
    let board = Board::load(conn, &run_id)?;
    let startable = board.startable();
    let mine = |t: &Task| t.claimable_by.as_deref() == Some(persona.as_str());
    // A replacement takes its predecessor's task first.
    let predecessor_task = match &w.previous_worker_id {
        Some(p) => workers::get(conn, p)?.task_id,
        None => None,
    };
    let pick = predecessor_task
        .as_deref()
        .and_then(|pt| startable.iter().find(|t| t.task_id == pt && mine(t)))
        .or_else(|| startable.iter().find(|t| mine(t)))
        .map(|t| (*t).clone());
    let Some(task) = pick else {
        let tell = format!(
            "horch tell orchestrator \"[{}] NOTE: No task for {}. I exit. PID {}.\"",
            w.label(),
            persona,
            w.supervisor_pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "unknown".into())
        );
        conn.execute(
            "UPDATE workers SET status = ?2, end_reason = ?3 WHERE worker_id = ?1",
            params![
                worker_id,
                WorkerStatus::NoTask.as_str(),
                format!("no task for persona {persona}")
            ],
        )?;
        util::event(
            conn,
            &About {
                run: Some(&run_id),
                task: None,
                worker: Some(worker_id),
            },
            Actor::Worker,
            EventKind::NoTask,
            &format!("No task for persona {persona}."),
        )?;
        return Ok(ClaimOutcome::NoTask {
            persona,
            role: w.role.clone(),
            supervisor_pid: w.supervisor_pid,
            tell,
        });
    };
    let now = util::now();
    let attempt = task.attempts + 1;
    let n = conn.execute(
        "UPDATE tasks SET status = 'running', current_worker_id = ?2, attempts = ?3, updated_at = ?4
         WHERE task_id = ?1 AND status = 'queued'",
        params![task.task_id, worker_id, attempt, now],
    )?;
    if n != 1 {
        bail!("{} was claimed by another worker", task.short());
    }
    conn.execute(
        "UPDATE workers SET task_id = ?2, attempt = ?3, status = 'working', claimed_at = ?4,
            last_progress_at = ?4, last_progress_source = 'agent', stall_rounds = 0
         WHERE worker_id = ?1",
        params![worker_id, task.task_id, attempt, now],
    )?;
    // An open worktree from an earlier attempt becomes this worker's.
    conn.execute(
        "UPDATE worktrees SET worker_id = ?2
         WHERE task_id = ?1 AND status IN ('open', 'landing', 'landed', 'cleanup_failed')",
        params![task.task_id, worker_id],
    )?;
    util::event(
        conn,
        &About {
            run: Some(&run_id),
            task: Some(&task.task_id),
            worker: Some(worker_id),
        },
        Actor::Worker,
        EventKind::Claimed,
        &format!(
            "{} claimed {} (attempt {attempt}).",
            w.label(),
            task.short()
        ),
    )?;
    let task = get(conn, &task.task_id)?;
    let w = workers::get(conn, worker_id)?;
    Ok(ClaimOutcome::Claimed(Box::new(claimed(conn, &w, task)?)))
}

fn claimed(conn: &Connection, w: &Worker, task: Task) -> Result<Claimed> {
    let prior = workers::attempts_at(conn, &task.task_id)?
        .into_iter()
        .filter(|a| a.worker_id != w.worker_id)
        .map(|a| PriorAttempt {
            worker_id: a.worker_id,
            role: a.role,
            attempt: a.attempt,
            status: a.status,
            end_reason: a.end_reason,
            summary: a.summary,
            session_id: a.session_id,
            transcript_path: a.transcript_path,
        })
        .collect();
    let wt: Option<(String, String)> = optional(conn.query_row(
        "SELECT path, branch FROM worktrees WHERE task_id = ?1
           AND status IN ('open', 'landing', 'landed', 'cleanup_failed')
         ORDER BY created_at DESC LIMIT 1",
        [&task.task_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    Ok(Claimed {
        own: files(conn, &task.task_id, FileMode::Own)?,
        do_not_touch: files(conn, &task.task_id, FileMode::DoNotTouch)?,
        attempt: w.attempt.unwrap_or(task.attempts),
        prior,
        worktree: wt.as_ref().map(|(p, _)| p.clone()),
        reopened_from: super::import::reopened_from(conn, &task.task_id)?,
        worktree_branch: wt.map(|(_, b)| b),
        task,
    })
}

/// Let the fallback persona claim `task`.
pub fn fallback(conn: &Connection, task: &Task, reason: &str) -> Result<Task> {
    let Some(fb) = &task.fallback_persona else {
        bail!("{} has no fallback persona", task.short());
    };
    if task.status != TaskStatus::Queued.as_str() {
        bail!(
            "{} is {}; only a queued task moves to its fallback",
            task.short(),
            task.status
        );
    }
    conn.execute(
        "UPDATE tasks SET claimable_by = ?2, fallback_reason = ?3, updated_at = ?4 WHERE task_id = ?1",
        params![task.task_id, fb, reason, util::now()],
    )?;
    util::event(
        conn,
        &About {
            run: Some(&task.run_id),
            task: Some(&task.task_id),
            worker: None,
        },
        Actor::Orchestrator,
        EventKind::Fallback,
        &format!("{} moves to fallback {fb}: {reason}", task.short()),
    )?;
    get(conn, &task.task_id)
}

/// Drop a task.
pub fn cancel(conn: &Connection, task: &Task, reason: &str) -> Result<Task> {
    if matches!(task.status.as_str(), "done" | "cancelled") {
        bail!("{} is already {}", task.short(), task.status);
    }
    conn.execute(
        "UPDATE tasks SET status = 'cancelled', summary = COALESCE(summary, ?2), updated_at = ?3,
            finished_at = ?3
         WHERE task_id = ?1",
        params![task.task_id, format!("cancelled: {reason}"), util::now()],
    )?;
    util::event(
        conn,
        &About {
            run: Some(&task.run_id),
            task: Some(&task.task_id),
            worker: task.current_worker_id.as_deref(),
        },
        Actor::Orchestrator,
        EventKind::Cancelled,
        &format!("{} cancelled: {reason}", task.short()),
    )?;
    get(conn, &task.task_id)
}

/// Put a task back in the queue after its worker died, first in line.
pub fn requeue(conn: &Connection, task_id: &str, why: &str) -> Result<()> {
    let task = get(conn, task_id)?;
    if !matches!(task.status.as_str(), "running" | "landing") {
        return Ok(());
    }
    conn.execute(
        "UPDATE tasks SET status = 'queued', current_worker_id = NULL,
            priority = priority - ?2, updated_at = ?3
         WHERE task_id = ?1",
        params![task_id, RETRY_PRIORITY_BOOST, util::now()],
    )?;
    util::event(
        conn,
        &About {
            run: Some(&task.run_id),
            task: Some(task_id),
            worker: task.current_worker_id.as_deref(),
        },
        Actor::Horch,
        EventKind::Requeued,
        &format!("{} is queued again, first in line: {why}", task.short()),
    )?;
    Ok(())
}

/// 1 persona's line of `horch ledger plan-workers`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersonaPlan {
    pub persona: String,
    pub startable: i64,
    pub on_the_way: i64,
    pub spawn: i64,
    /// Workers being replaced: spawn these with `--replaces`.
    pub replaces: Vec<String>,
}

/// How many workers of each persona to spawn now: the tasks that can start
/// now, minus the workers of that persona still spawning, starting or
/// claiming.
pub fn plan_workers(conn: &Connection, run_id: &str) -> Result<Vec<PersonaPlan>> {
    let startable = startable(conn, run_id)?;
    let mut by: BTreeMap<String, (i64, Vec<String>)> = BTreeMap::new();
    for t in &startable {
        let Some(p) = &t.claimable_by else { continue };
        let entry = by.entry(p.clone()).or_default();
        entry.0 += 1;
        // A task whose last attempt died names that worker for --replaces.
        if let Some(last) = workers::attempts_at(conn, &t.task_id)?.last() {
            if last.status == WorkerStatus::Incomplete.as_str() {
                entry.1.push(last.worker_id.clone());
            }
        }
    }
    let mut out = Vec::new();
    for (persona, (n, replaces)) in by {
        let on_the_way: i64 = conn.query_row(
            "SELECT count(*) FROM workers WHERE run_id = ?1 AND persona = ?2 AND kind = 'worker'
               AND status IN ('spawning', 'starting', 'claiming')",
            params![run_id, persona],
            |r| r.get(0),
        )?;
        out.push(PersonaPlan {
            spawn: (n - on_the_way).max(0),
            persona,
            startable: n,
            on_the_way,
            replaces,
        });
    }
    Ok(out)
}

/// Whether every task of the run is finished.
pub fn all_finished(conn: &Connection, run_id: &str) -> Result<bool> {
    let open: i64 = conn.query_row(
        "SELECT count(*) FROM tasks WHERE run_id = ?1 AND status NOT IN ('done', 'cancelled')",
        [run_id],
        |r| r.get(0),
    )?;
    let total: i64 = conn.query_row(
        "SELECT count(*) FROM tasks WHERE run_id = ?1",
        [run_id],
        |r| r.get(0),
    )?;
    Ok(total > 0 && open == 0)
}
