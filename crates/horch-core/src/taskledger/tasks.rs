//! Tasks: queueing, the claimable rule and the claim (sections 4.3 to 4.5,
//! 5.1, 5.2).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Row, Transaction};
use serde::Serialize;

use super::model::{
    Actor, EndReason, EventKind, FileMode, FindingStatus, HeldReason, Origin, OutboxEvent,
    ReviewMode, StepName, TaskKind, TaskStatus, WorkKind, WorkerKind, WorkerStatus, WorktreeMode,
    WorktreeStatus,
};
use super::outbox::{self, Wake};
use super::runs::Run;
use super::sql_words::{self, word};
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
    /// The ticket's `**Kind:**` (C1).
    pub work_kind: Option<String>,
    /// The ticket's `**Review:**`, or the default of its `Kind` (C1).
    pub review_mode: Option<String>,
    /// The ticket text as `add-map` queued it. A claim prints it; status
    /// JSON leaves it out.
    #[serde(skip_serializing)]
    pub ticket_snapshot: Option<String>,
    /// The sha256 of `ticket_snapshot`, in hex.
    pub ticket_sha: Option<String>,
    /// The ticket says `**Check:** none`: it has no check on purpose.
    pub check_none: bool,
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

    /// Parked by `done --unlanded`: queued, held, with its worktree kept
    /// (trace-fixes D16). `requeue` resumes it on that worktree.
    pub fn is_parked(&self) -> bool {
        self.status == TaskStatus::Queued.as_str()
            && self.held_reason.as_deref() == Some(HeldReason::Unlanded.as_str())
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
            work_kind: r.get("work_kind")?,
            review_mode: r.get("review_mode")?,
            ticket_snapshot: r.get("ticket_snapshot")?,
            ticket_sha: r.get("ticket_sha")?,
            check_none: r.get::<_, i64>("check_none")? != 0,
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
    /// The ticket's `**Kind:**` (C1).
    pub work_kind: Option<WorkKind>,
    /// The ticket's review mode: explicit, else the default of its `Kind`.
    pub review_mode: Option<ReviewMode>,
    /// The ticket text at queue time; its sha256 is stored with it.
    pub ticket_snapshot: Option<String>,
    /// `**Check:** none`: no check on purpose.
    pub check_none: bool,
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
            work_kind: None,
            review_mode: None,
            ticket_snapshot: None,
            check_none: false,
        }
    }
}

/// The sha256 of `text`, in lowercase hex.
pub fn ticket_sha(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The tasks of `run_id` that already queue one of `files` and are not
/// cancelled. `add-map` refuses these unless it is told `--again` (TRF-13).
pub fn already_queued(conn: &Connection, run_id: &str, files: &[&str]) -> Result<Vec<Task>> {
    Ok(of_run(conn, run_id)?
        .into_iter()
        .filter(|t| t.status != TaskStatus::Cancelled.as_str())
        .filter(|t| t.task_file.as_deref().is_some_and(|f| files.contains(&f)))
        .collect())
}

/// Refuse `items` when one of their tickets is already a task in `run_id`
/// that is not cancelled. The error names each such task and `--again`.
pub fn refuse_queued(conn: &Connection, run_id: &str, items: &[(String, NewTask)]) -> Result<()> {
    let files: Vec<&str> = items.iter().map(|(_, t)| t.task_file.as_str()).collect();
    let queued = already_queued(conn, run_id, &files)?;
    if queued.is_empty() {
        return Ok(());
    }
    let lines: Vec<String> = queued
        .iter()
        .map(|t| {
            format!(
                "{} is already {} ({}) in this run.",
                t.task_file.as_deref().unwrap_or("-"),
                t.short(),
                t.status
            )
        })
        .collect();
    bail!(
        "{}\nNothing was queued. To queue these tickets again, add --again.",
        lines.join("\n")
    )
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

/// The open tasks that wait for `task_id`, directly or through another
/// task, in `seq` order.
pub fn dependents(conn: &Connection, task_id: &str) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "WITH RECURSIVE down(id) AS (
            SELECT task_id FROM task_deps WHERE blocked_by = ?1
            UNION SELECT d.task_id FROM task_deps d JOIN down ON d.blocked_by = down.id)
         SELECT t.* FROM tasks t JOIN down ON t.task_id = down.id
         WHERE t.status NOT IN ({ended}) ORDER BY t.seq",
        ended = sql_words::list(sql_words::TASKS_ENDED)
    ))?;
    let rows = stmt
        .query_map([task_id], Task::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// `horch ledger unblock`: `task` no longer waits for `blocker`.
pub fn unblock(conn: &Connection, task: &Task, blocker: &Task) -> Result<()> {
    let n = conn.execute(
        "DELETE FROM task_deps WHERE task_id = ?1 AND blocked_by = ?2",
        params![task.task_id, blocker.task_id],
    )?;
    if n == 0 {
        bail!("{} does not wait for {}", task.short(), blocker.short());
    }
    util::event(
        conn,
        &About {
            run: Some(&task.run_id),
            task: Some(&task.task_id),
            worker: None,
        },
        Actor::Orchestrator,
        EventKind::Note,
        &format!("{} no longer waits for {}.", task.short(), blocker.short()),
    )?;
    Ok(())
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
    let edges = dependency_edges(conn, run, items, &keys)?;
    if let Some(cycle) = find_cycle(&edges) {
        let names: Vec<&str> = cycle.iter().map(|&i| items[i].0.as_str()).collect();
        bail!("the map has a dependency cycle: {}", names.join(" -> "));
    }
    let ids = insert_in_order(conn, run, items, &keys, &topo_order(&edges))?;
    let mut out = queued_events(conn, run, items, ids, actor)?;
    out.sort_by_key(|t| t.seq);
    Ok(out)
}

/// The edges inside the map (node -> the nodes it waits for). Blockers
/// outside the map must already exist.
fn dependency_edges(
    conn: &Connection,
    run: &Run,
    items: &[(String, NewTask)],
    keys: &BTreeMap<&str, usize>,
) -> Result<Vec<Vec<usize>>> {
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
    Ok(edges)
}

/// Insert in dependency order so every edge points at an existing row.
/// Returns the new id of each item, by index.
fn insert_in_order(
    conn: &Connection,
    run: &Run,
    items: &[(String, NewTask)],
    keys: &BTreeMap<&str, usize>,
    order: &[usize],
) -> Result<Vec<Option<String>>> {
    let mut ids: Vec<Option<String>> = vec![None; items.len()];
    for &i in order {
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
    Ok(ids)
}

/// Record the `Added` event of every inserted item and return the tasks.
fn queued_events(
    conn: &Connection,
    run: &Run,
    items: &[(String, NewTask)],
    ids: Vec<Option<String>>,
    actor: Actor,
) -> Result<Vec<Task>> {
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
            worktree_mode, land_check, kind, parent_task_id, status, origin, created_at, updated_at,
            work_kind, review_mode, ticket_snapshot, ticket_sha, check_none)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20, ?20, ?21, ?22, ?23, ?24, ?25)",
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
            new.work_kind.map(WorkKind::as_str),
            new.review_mode.map(ReviewMode::as_str),
            new.ticket_snapshot,
            new.ticket_snapshot.as_deref().map(ticket_sha),
            new.check_none as i64,
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
pub fn find_cycle(edges: &[Vec<usize>]) -> Option<Vec<usize>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Open,
        Closed,
    }
    fn visit(n: usize, edges: &[Vec<usize>], mark: &mut [Mark], path: &mut Vec<usize>) -> bool {
        mark[n] = Mark::Open;
        path.push(n);
        let found = edges[n].iter().any(|&m| match mark[m] {
            Mark::Open => {
                let at = path.iter().position(|&p| p == m).unwrap_or(0);
                path.drain(..at);
                path.push(m);
                true
            }
            Mark::New => visit(m, edges, mark, path),
            Mark::Closed => false,
        });
        if !found {
            path.pop();
            mark[n] = Mark::Closed;
        }
        found
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
    /// A blocker was cancelled: it never finishes unless someone acts (#119).
    BlockerCancelled { task: String },
    /// A blocker is parked unlanded: it resumes only on a requeue (D16).
    BlockerParked { task: String },
    /// An exclusive task runs, or this exclusive task waits for others.
    Exclusive { running: String },
    /// A running task owns 1 of this task's files.
    FileHeld { path: String, by: String },
    /// Another shared-checkout task works in the 1 shared checkout.
    SharedCheckout { running: String },
}

impl std::fmt::Display for Wait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Wait::BlockedBy { task, status } => write!(f, "blocked by {task} ({status})"),
            Wait::BlockerCancelled { task } => {
                write!(
                    f,
                    "{task} was cancelled: requeue it, or drop the dependency"
                )
            }
            Wait::BlockerParked { task } => write!(
                f,
                "{task} is parked unlanded. Run horch ledger requeue {task} to resume it"
            ),
            Wait::Exclusive { running } => write!(f, "exclusive: waits for {running}"),
            Wait::FileHeld { path, by } => write!(f, "{path} is held by {by}"),
            Wait::SharedCheckout { running } => {
                write!(f, "exclusive: shared checkout, waits for {running}")
            }
        }
    }
}

/// The 2 text columns of `sql`, a query of 1 `?1` run id.
fn pairs(conn: &Connection, sql: &str, run_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([run_id], |r| r.try_into())?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// An exclusive task runs, or `t` is exclusive and others run.
fn exclusive(t: &Task, work: &[&&Task]) -> Option<Wait> {
    let blocker = work.iter().find(|a| !a.parallel).or_else(|| {
        // An exclusive task waits for any running one.
        (!t.parallel).then(|| work.first()).flatten()
    });
    blocker.map(|x| Wait::Exclusive { running: x.short() })
}

/// 1 checkout holds the edits of every shared task: they go 1 by 1.
fn shared_checkout(t: &Task, work: &[&&Task]) -> Option<Wait> {
    if t.uses_worktree() {
        return None;
    }
    work.iter()
        .find(|a| !a.uses_worktree())
        .map(|a| Wait::SharedCheckout { running: a.short() })
}

/// The state the claimable rule reads: the run's tasks with their files and
/// blockers, and the personas that have a live worker.
struct Board {
    tasks: Vec<Task>,
    own: BTreeMap<String, Vec<String>>,
    deps: BTreeMap<String, Vec<String>>,
    staffed: BTreeSet<String>,
}

impl Board {
    fn load(conn: &Connection, run_id: &str) -> Result<Board> {
        let tasks = of_run(conn, run_id)?;
        let mut own: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (t, p) in pairs(
            conn,
            "SELECT f.task_id, f.path FROM task_files f JOIN tasks t ON t.task_id = f.task_id
             WHERE t.run_id = ?1 AND f.mode = 'own'",
            run_id,
        )? {
            own.entry(t).or_default().push(p);
        }
        let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (t, b) in pairs(
            conn,
            "SELECT d.task_id, d.blocked_by FROM task_deps d JOIN tasks t ON t.task_id = d.task_id
             WHERE t.run_id = ?1",
            run_id,
        )? {
            deps.entry(t).or_default().push(b);
        }
        let staffed = workers::of_run(conn, run_id)?
            .into_iter()
            .filter(|w| w.kind == WorkerKind::Worker.as_str())
            .filter(|w| w.status().is_ok_and(WorkerStatus::is_live))
            .filter_map(|w| w.persona)
            .collect();
        Ok(Board {
            tasks,
            own,
            deps,
            staffed,
        })
    }

    /// Whether a live or starting worker can claim `t` (TRF-31).
    fn is_staffed(&self, t: &Task) -> bool {
        t.claimable_by
            .as_deref()
            .is_some_and(|p| self.staffed.contains(p))
    }

    fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.task_id == id)
    }

    fn is_active(t: &Task) -> bool {
        matches!(t.status.as_str(), "running" | "landing")
    }

    /// Why `t` cannot start beside the `active` tasks, or `None`. Review
    /// tasks change no code: they never conflict on files and never wait for
    /// an exclusive task (that task waits for them).
    fn wait(&self, t: &Task, active: &[&Task]) -> Option<Wait> {
        self.blocked_by(t).or_else(|| {
            let work: Vec<&&Task> = active.iter().filter(|a| !a.is_review()).collect();
            (!t.is_review())
                .then(|| {
                    exclusive(t, &work)
                        .or_else(|| shared_checkout(t, &work))
                        .or_else(|| self.file_held(t, &work))
                })
                .flatten()
        })
    }

    /// The first blocker of `t` that is not done.
    fn blocked_by(&self, t: &Task) -> Option<Wait> {
        self.deps
            .get(&t.task_id)
            .into_iter()
            .flatten()
            .filter_map(|b| self.task(b))
            .find_map(|bt| {
                if bt.status == TaskStatus::Cancelled.as_str() {
                    Some(Wait::BlockerCancelled { task: bt.short() })
                } else if bt.is_parked() {
                    Some(Wait::BlockerParked { task: bt.short() })
                } else if bt.status != TaskStatus::Done.as_str() {
                    Some(Wait::BlockedBy {
                        task: bt.short(),
                        status: bt.status.clone(),
                    })
                } else {
                    None
                }
            })
    }

    /// The first file of `t` that a running task owns.
    fn file_held(&self, t: &Task, work: &[&&Task]) -> Option<Wait> {
        let mine = self.own.get(&t.task_id).cloned().unwrap_or_default();
        work.iter().find_map(|a| {
            self.own
                .get(&a.task_id)
                .into_iter()
                .flatten()
                .find_map(|theirs| {
                    mine.iter()
                        .find(|m| util::paths_overlap(m, theirs))
                        .map(|p| Wait::FileHeld {
                            path: p.clone(),
                            by: a.short(),
                        })
                })
        })
    }

    /// Queued tasks in claim order: priority, then seq. A held task waits
    /// for the operator, not for a worker.
    fn queued(&self) -> Vec<&Task> {
        let mut q: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Queued.as_str() && t.held_reason.is_none())
            .collect();
        q.sort_by_key(|t| (t.priority, t.seq));
        q
    }

    /// Every queued task in claim order, with why it cannot start (`None`
    /// when it can). The tasks are chosen greedily: each one is checked
    /// against the running tasks and the startable ones before it. A
    /// startable task holds its slot only when its persona has a live or
    /// starting worker: no task waits behind one that nobody will claim.
    fn plan(&self) -> Vec<(&Task, Option<Wait>)> {
        let mut active: Vec<&Task> = self.tasks.iter().filter(|t| Self::is_active(t)).collect();
        let mut out = Vec::new();
        for t in self.queued() {
            let w = self.wait(t, &active);
            if w.is_none() && self.is_staffed(t) {
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
    /// The commit the inherited worktree already landed, if it did.
    pub landed_commit: Option<String>,
    /// The finish steps after `land`: what a landed worktree has left.
    pub steps_after_land: Vec<String>,
    /// The ticket text as `add-map` queued it (TRF-12).
    pub ticket_snapshot: Option<String>,
    /// The handoff file of the last attempt, when it ended through
    /// `horch ledger handoff` (C4). The claim prints it first.
    pub handoff_path: Option<String>,
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
    let (run_id, persona) = check_claimer(&w)?;
    match held_task(conn, &w)? {
        Some(task) => claimed_outcome(conn, &w, task),
        None => claim_next(conn, &w, &run_id, &persona),
    }
}

/// A second claim by a working worker gives back its own task.
fn held_task(conn: &Connection, w: &Worker) -> Result<Option<Task>> {
    match (&w.task_id, w.status()? == WorkerStatus::Working) {
        (Some(t), true) => Ok(Some(get(conn, t)?)),
        _ => Ok(None),
    }
}

/// Take the next task for `w`, or end `w` when the queue has none for it.
fn claim_next(conn: &Connection, w: &Worker, run_id: &str, persona: &str) -> Result<ClaimOutcome> {
    let Some(task) = pick_task(conn, w, run_id, persona)? else {
        return no_task(conn, w, persona, run_id);
    };
    take_task(conn, w, run_id, &task)?;
    let task = get(conn, &task.task_id)?;
    let w = workers::get(conn, &w.worker_id)?;
    claimed_outcome(conn, &w, task)
}

/// The claimed outcome for `task`, held by `w`.
fn claimed_outcome(conn: &Connection, w: &Worker, task: Task) -> Result<ClaimOutcome> {
    Ok(ClaimOutcome::Claimed(Box::new(claimed(conn, w, task)?)))
}

/// Refuse a claimer that cannot claim; give back its run and persona.
fn check_claimer(w: &Worker) -> Result<(String, String)> {
    if w.kind == WorkerKind::Orchestrator.as_str() {
        bail!("the orchestrator does not claim tasks");
    }
    let status = w.status()?;
    let run_id = w.run_id.clone().context("this worker has no run")?;
    let persona = w.persona.clone().context("this worker has no persona")?;
    if !status.is_live() {
        bail!(
            "worker {} is {}; a worker that ended cannot claim",
            w.label(),
            w.status
        );
    }
    Ok((run_id, persona))
}

/// The next startable task of `persona`. A worker spawned for a target
/// task takes that task first, if it is still startable (TRF-24). A
/// replacement takes its predecessor's task first. No worker takes a task
/// that another live, unclaimed worker was spawned for.
fn pick_task(conn: &Connection, w: &Worker, run_id: &str, persona: &str) -> Result<Option<Task>> {
    let board = Board::load(conn, run_id)?;
    let startable = board.startable();
    if let Some(target) = &w.target_task_id {
        if let Some(t) = startable.iter().find(|t| &t.task_id == target) {
            return Ok(Some((*t).clone()));
        }
    }
    let reserved = reserved_for_others(conn, w, run_id)?;
    let mine =
        |t: &&Task| t.claimable_by.as_deref() == Some(persona) && !reserved.contains(&t.task_id);
    let predecessor = predecessor_task(conn, w)?;
    let first = |pred: Option<&str>| {
        startable
            .iter()
            .copied()
            .filter(mine)
            .find(|t| pred.is_none_or(|p| t.task_id == p))
    };
    Ok(first(predecessor.as_deref())
        .or_else(|| first(None))
        .cloned())
}

/// The tasks that other workers of `run_id` were spawned for. A reservation
/// lasts while its worker is live and has not claimed (TRF-24): it ends
/// when the worker ends or claims, so no dead worker holds a task.
fn reserved_for_others(conn: &Connection, w: &Worker, run_id: &str) -> Result<BTreeSet<String>> {
    Ok(reservations(conn, run_id, Some(&w.worker_id))?
        .into_values()
        .collect())
}

/// The reservations of `run_id` (TRF-24): each live worker with no claim
/// that was spawned for a task, with that task. `except` names a worker
/// whose reservation does not count.
fn reservations(
    conn: &Connection,
    run_id: &str,
    except: Option<&str>,
) -> Result<BTreeMap<String, String>> {
    Ok(workers::of_run(conn, run_id)?
        .into_iter()
        .filter(|o| Some(o.worker_id.as_str()) != except && o.claimed_at.is_none())
        .filter(|o| o.status().is_ok_and(WorkerStatus::is_live))
        .filter_map(|o| o.target_task_id.map(|t| (o.worker_id, t)))
        .collect())
}

/// The task a replaced worker held, if `w` replaces one.
fn predecessor_task(conn: &Connection, w: &Worker) -> Result<Option<String>> {
    let prev = w.previous_worker_id.as_deref();
    Ok(prev
        .map(|p| workers::get(conn, p))
        .transpose()?
        .and_then(|p| p.task_id))
}

/// End a worker that found no task: the notice, the status and the event.
fn no_task(conn: &Connection, w: &Worker, persona: &str, run_id: &str) -> Result<ClaimOutcome> {
    let tell = format!(
        "horch tell orchestrator \"[{}] NOTE: No task for {}. I exit. PID {}.\"",
        w.label(),
        persona,
        w.supervisor_pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "unknown".into())
    );
    let now = util::now();
    conn.execute(
        "UPDATE workers SET status = ?2, end_reason = ?3, ended_at = COALESCE(ended_at, ?4)
         WHERE worker_id = ?1",
        params![
            w.worker_id,
            WorkerStatus::NoTask.as_str(),
            format!("no task for persona {persona}"),
            now
        ],
    )?;
    no_task_wake(conn, w, persona, run_id, &now)?;
    util::event(
        conn,
        &About {
            run: Some(run_id),
            task: None,
            worker: Some(&w.worker_id),
        },
        Actor::Worker,
        EventKind::NoTask,
        &format!("No task for persona {persona}."),
    )?;
    Ok(ClaimOutcome::NoTask {
        persona: persona.to_string(),
        role: w.role.clone(),
        supervisor_pid: w.supervisor_pid,
        tell,
    })
}

/// The `no-task` outbox row that tells the orchestrator a worker ended
/// without a task (trace-fixes C3). 1 row for each worker.
fn no_task_wake(
    conn: &Connection,
    w: &Worker,
    persona: &str,
    run_id: &str,
    now: &str,
) -> Result<()> {
    let event = OutboxEvent::NoTask.as_str();
    conn.execute(
        "INSERT OR IGNORE INTO outbox (id, run_id, event, key, text, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            util::new_id(),
            run_id,
            event,
            format!("{run_id}:{event}:{}", w.worker_id),
            format!(
                "[horch] NOTE: Worker {} found no task for persona {persona} and ended. \
                 Run horch ledger plan-workers before you spawn again.",
                w.label()
            ),
            now
        ],
    )?;
    Ok(())
}

/// Write the claim: the task, the worker, an open worktree from an earlier
/// attempt, and the event.
fn take_task(conn: &Connection, w: &Worker, run_id: &str, task: &Task) -> Result<()> {
    let worker_id = w.worker_id.as_str();
    let now = util::now();
    let attempt = task.attempts + 1;
    let n = conn.execute(
        &format!(
            "UPDATE tasks SET status = {running}, current_worker_id = ?2, attempts = ?3, updated_at = ?4
             WHERE task_id = ?1 AND status = {queued}",
            running = word(TaskStatus::Running),
            queued = word(TaskStatus::Queued)
        ),
        params![task.task_id, worker_id, attempt, now],
    )?;
    if n != 1 {
        bail!("{} was claimed by another worker", task.short());
    }
    conn.execute(
        &format!(
            "UPDATE workers SET task_id = ?2, attempt = ?3, status = {working}, claimed_at = ?4,
                last_progress_at = ?4, last_progress_source = 'agent', stall_rounds = 0
             WHERE worker_id = ?1",
            working = word(WorkerStatus::Working)
        ),
        params![worker_id, task.task_id, attempt, now],
    )?;
    // An open worktree from an earlier attempt becomes this worker's.
    conn.execute(
        &format!(
            "UPDATE worktrees SET worker_id = ?2 WHERE task_id = ?1 AND status IN ({held})",
            held = sql_words::list(sql_words::WORKTREES_HELD)
        ),
        params![task.task_id, worker_id],
    )?;
    util::event(
        conn,
        &About {
            run: Some(run_id),
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
    )
}

fn claimed(conn: &Connection, w: &Worker, task: Task) -> Result<Claimed> {
    let attempts: Vec<Worker> = workers::attempts_at(conn, &task.task_id)?
        .into_iter()
        .filter(|a| a.worker_id != w.worker_id)
        .collect();
    let handoff_path = attempts
        .last()
        .filter(|a| a.end_reason.as_deref() == Some(EndReason::HandedOff.as_str()))
        .and_then(|a| a.handoff_path.clone());
    let prior = attempts
        .into_iter()
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
    let wt: Option<(String, String, Option<String>)> = optional(conn.query_row(
        &format!(
            "SELECT path, branch, CASE WHEN status IN ({landed}) THEN landed_commit END
             FROM worktrees WHERE task_id = ?1 AND status IN ({held})
             ORDER BY created_at DESC LIMIT 1",
            landed = sql_words::list(sql_words::WORKTREES_LANDED),
            held = sql_words::list(sql_words::WORKTREES_HELD)
        ),
        [&task.task_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ))?;
    let steps = super::runs::get(conn, &task.run_id)?.steps()?;
    let steps_after_land = steps
        .iter()
        .skip_while(|s| **s != StepName::Land)
        .skip(1)
        .map(|s| s.as_str().to_string())
        .collect();
    Ok(Claimed {
        own: files(conn, &task.task_id, FileMode::Own)?,
        do_not_touch: files(conn, &task.task_id, FileMode::DoNotTouch)?,
        attempt: w.attempt.unwrap_or(task.attempts),
        prior,
        worktree: wt.as_ref().map(|(p, _, _)| p.clone()),
        reopened_from: super::import::reopened_from(conn, &task.task_id)?,
        landed_commit: wt.as_ref().and_then(|(_, _, l)| l.clone()),
        steps_after_land,
        worktree_branch: wt.map(|(_, b, _)| b),
        ticket_snapshot: task.ticket_snapshot.clone(),
        handoff_path,
        task,
    })
}

/// What [`handoff`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct HandedOff {
    /// The task, queued again first in line.
    pub task: Task,
    /// The worker, ended `handed_off`.
    pub worker: Worker,
    /// The new `startable` outbox row, if its key was new.
    pub wake_id: Option<String>,
}

/// `horch ledger handoff` (trace-fixes C4), in the caller's transaction:
/// the worker ends `handed_off` with `path`, its task is queued again in
/// front of every other queued task and keeps its worktree, and a
/// `startable` wake is written. `path` must exist.
pub fn handoff(tx: &Transaction, worker_id: &str, path: &str) -> Result<HandedOff> {
    let w = workers::get(tx, worker_id)?;
    let task = handoff_task(tx, &w)?;
    if !std::path::Path::new(path).is_file() {
        bail!("the handoff file {path} does not exist; write it first");
    }
    workers::hand_off(tx, worker_id, path)?;
    tx.execute(
        &format!(
            "UPDATE tasks SET status = {queued}, current_worker_id = NULL, held_reason = NULL,
                priority = MIN(priority, (SELECT COALESCE(MIN(o.priority), tasks.priority)
                    FROM tasks o WHERE o.run_id = tasks.run_id AND o.status = {queued}
                    AND o.task_id != tasks.task_id) - 1),
                updated_at = ?2
             WHERE task_id = ?1",
            queued = word(TaskStatus::Queued)
        ),
        params![task.task_id, util::now()],
    )?;
    // The worktree waits for the next claim, which takes it over.
    tx.execute(
        &format!(
            "UPDATE worktrees SET worker_id = NULL WHERE task_id = ?1 AND status IN ({held})",
            held = sql_words::list(sql_words::WORKTREES_HELD)
        ),
        [&task.task_id],
    )?;
    util::event(
        tx,
        &About {
            run: Some(&task.run_id),
            task: Some(&task.task_id),
            worker: Some(worker_id),
        },
        Actor::Worker,
        EventKind::Requeued,
        &format!(
            "{} handed off {}: it is queued again, first in line, with its worktree. Handoff: {path}",
            w.label(),
            task.short()
        ),
    )?;
    let wake_id = outbox::write(tx, &task.run_id, &handoff_wake(&w, &task, path))?;
    Ok(HandedOff {
        task: get(tx, &task.task_id)?,
        worker: workers::get(tx, worker_id)?,
        wake_id,
    })
}

/// The running task `w` holds: the precondition of a handoff.
fn handoff_task(conn: &Connection, w: &Worker) -> Result<Task> {
    let Some(task_id) = &w.task_id else {
        bail!(
            "worker {} holds no task; only a worker with a running task hands off",
            w.label()
        );
    };
    let task = get(conn, task_id)?;
    if w.status()? != WorkerStatus::Working
        || task.status != TaskStatus::Running.as_str()
        || task.current_worker_id.as_deref() != Some(w.worker_id.as_str())
    {
        bail!(
            "{} is {} and worker {} is {}; only a worker with a running task hands off",
            task.short(),
            task.status,
            w.label(),
            w.status
        );
    }
    Ok(task)
}

/// The `startable` wake of a handoff: spawn 1 replacement.
fn handoff_wake(w: &Worker, task: &Task, path: &str) -> Wake {
    let persona = task.claimable_by.as_deref().unwrap_or("?");
    Wake::new(
        OutboxEvent::Startable,
        format!("handoff:{}", w.worker_id),
        format!(
            "[horch] NOTE: {} handed off {}. {} is queued first with its worktree. \
             The handoff is {path}. Spawn: {persona} x1 (--replaces {}).",
            w.label(),
            task.short(),
            task.short(),
            w.label()
        ),
    )
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

/// Drop a task: the 1 cancel path (#136). In the caller's transaction the
/// task lets go of its worker, the worker row ends `stopped`, and an open
/// worktree becomes `abandoned` (its branch and folder stay, so a requeue
/// can reopen it). The worker's process is not signalled here: after the
/// commit, the caller stops it with [`super::stop::stop_cancelled`] (TRF-36).
pub fn cancel(conn: &Connection, task: &Task, reason: &str) -> Result<Task> {
    if matches!(task.status.as_str(), "done" | "cancelled") {
        bail!("{} is already {}", task.short(), task.status);
    }
    let why = format!("task cancelled: {reason}");
    conn.execute(
        &format!(
            "UPDATE tasks SET status = {cancelled}, summary = COALESCE(summary, ?2), updated_at = ?3,
                finished_at = ?3, current_worker_id = NULL, held_reason = NULL
             WHERE task_id = ?1",
            cancelled = word(TaskStatus::Cancelled)
        ),
        params![task.task_id, format!("cancelled: {reason}"), util::now()],
    )?;
    if let Some(w) = &task.current_worker_id {
        workers::end(conn, w, WorkerStatus::Stopped, &why, None)?;
    }
    let open: Vec<String> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT worktree_id FROM worktrees WHERE task_id = ?1 AND status IN ({open})",
            open = sql_words::list(sql_words::WORKTREES_OPEN)
        ))?;
        let ids = stmt
            .query_map([&task.task_id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        ids
    };
    for id in open {
        super::finish::set_worktree(
            conn,
            &id,
            super::model::WorktreeStatus::Abandoned,
            Some(&why),
        )?;
    }
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

/// What [`requeue`] did with a task whose worker ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requeue {
    /// The task was not running: nothing changed.
    Skipped,
    /// Queued again, first in line.
    Queued,
    /// Its attempts since the last operator requeue reached the run's
    /// `max_attempts`: held for the operator (#168).
    Held,
}

/// Put a task back in the queue after its worker died, first in line, or
/// hold it when it used up the run's `max_attempts`.
pub fn requeue(conn: &Connection, task_id: &str, why: &str) -> Result<Requeue> {
    let task = get(conn, task_id)?;
    if !matches!(task.status.as_str(), "running" | "landing") {
        return Ok(Requeue::Skipped);
    }
    let about = About {
        run: Some(&task.run_id),
        task: Some(task_id),
        worker: task.current_worker_id.as_deref(),
    };
    let max = super::runs::get(conn, &task.run_id)?.max_attempts;
    let used = task.attempts - task.attempt_floor;
    if used >= max {
        let held = format!("{used} attempts failed");
        conn.execute(
            &format!(
                "UPDATE tasks SET status = {queued}, current_worker_id = NULL, held_reason = ?2,
                    updated_at = ?3
                 WHERE task_id = ?1",
                queued = word(TaskStatus::Queued)
            ),
            params![task_id, held, util::now()],
        )?;
        util::event(
            conn,
            &about,
            Actor::Horch,
            EventKind::Blocked,
            &format!(
                "{} is held for the operator: {held} ({}). Release it with horch ledger requeue {} \"<reason>\".",
                task.short(),
                attempt_ends(conn, &get(conn, task_id)?)?.join("; "),
                task.short()
            ),
        )?;
        return Ok(Requeue::Held);
    }
    conn.execute(
        &format!(
            "UPDATE tasks SET status = {queued}, current_worker_id = NULL,
                priority = priority - ?2, updated_at = ?3
             WHERE task_id = ?1",
            queued = word(TaskStatus::Queued)
        ),
        params![task_id, RETRY_PRIORITY_BOOST, util::now()],
    )?;
    util::event(
        conn,
        &about,
        Actor::Horch,
        EventKind::Requeued,
        &format!("{} is queued again, first in line: {why}", task.short()),
    )?;
    Ok(Requeue::Queued)
}

/// How each attempt at `task` since the last operator requeue ended, oldest
/// first: the worker's end reason, else its status.
pub fn attempt_ends(conn: &Connection, task: &Task) -> Result<Vec<String>> {
    Ok(workers::attempts_at(conn, &task.task_id)?
        .into_iter()
        .filter(|w| w.attempt.is_some_and(|a| a > task.attempt_floor))
        .map(|w| w.end_reason.unwrap_or(w.status))
        .collect())
}

/// `horch ledger requeue`: the operator wants a held or cancelled task
/// worked on again. Its attempt history stays, and it gets the run's
/// `max_attempts` anew. A cancelled task gets back the worktree its cancel
/// abandoned; a parked task resumes on the worktree it parked with.
pub fn requeue_by_operator(conn: &Connection, task: &Task, reason: &str) -> Result<Task> {
    let held = task.status == TaskStatus::Queued.as_str() && task.held_reason.is_some();
    if !held && task.status != TaskStatus::Cancelled.as_str() {
        bail!(
            "{} is {}; only a held or cancelled task is requeued",
            task.short(),
            task.status
        );
    }
    let now = util::now();
    if task.is_parked() {
        resume_parked_worktree(conn, task)?;
    }
    conn.execute(
        &format!(
            "UPDATE tasks SET status = {queued}, held_reason = NULL, attempt_floor = attempts,
                current_worker_id = NULL, summary = CASE WHEN status = {cancelled} THEN NULL ELSE summary END,
                finished_at = NULL, updated_at = ?2
             WHERE task_id = ?1",
            queued = word(TaskStatus::Queued),
            cancelled = word(TaskStatus::Cancelled)
        ),
        params![task.task_id, now],
    )?;
    // The newest abandoned worktree whose folder is still there.
    let kept: Option<(String, String)> = optional(conn.query_row(
        &format!(
            "SELECT worktree_id, path FROM worktrees WHERE task_id = ?1 AND status = {abandoned}
             ORDER BY created_at DESC, rowid DESC LIMIT 1",
            abandoned = word(WorktreeStatus::Abandoned)
        ),
        [&task.task_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    if let Some((id, _)) = kept.filter(|(_, p)| std::path::Path::new(p).exists()) {
        conn.execute(
            &format!(
                "UPDATE worktrees SET status = {open}, worker_id = NULL, reason = NULL WHERE worktree_id = ?1",
                open = word(WorktreeStatus::Open)
            ),
            [&id],
        )?;
        conn.execute(
            &format!(
                "UPDATE import_findings SET status = {requeued}, decided_by = 'orchestrator',
                    decided_at = ?2, decision_note = ?3
                 WHERE task_id = ?1 AND status = {open} AND kind = 'task_cancelled_open_worktree'",
                requeued = word(FindingStatus::Requeued),
                open = word(FindingStatus::Open)
            ),
            params![task.task_id, now, reason],
        )?;
    }
    util::event(
        conn,
        &About {
            run: Some(&task.run_id),
            task: Some(&task.task_id),
            worker: None,
        },
        Actor::Orchestrator,
        EventKind::Requeued,
        &format!("{} is queued again by the operator: {reason}", task.short()),
    )?;
    get(conn, &task.task_id)
}

/// The worktree a parked task kept becomes open again, for the next claim.
fn resume_parked_worktree(conn: &Connection, task: &Task) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE worktrees SET status = {open}, worker_id = NULL, reason = NULL
             WHERE task_id = ?1 AND status = {unlanded}",
            open = word(WorktreeStatus::Open),
            unlanded = word(WorktreeStatus::Unlanded)
        ),
        [&task.task_id],
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
/// claiming and not stale ([`workers::is_stale`]). A task that a live
/// worker with no claim was spawned for has its worker on the way already
/// (TRF-51): it needs no spawn, and that worker covers no other task.
pub fn plan_workers(conn: &Connection, run_id: &str) -> Result<Vec<PersonaPlan>> {
    plan_workers_except(conn, run_id, None)
}

/// [`plan_workers`], where the reservation of worker `except` does not
/// count: the spawn gate asks it for the worker it gates.
fn plan_workers_except(
    conn: &Connection,
    run_id: &str,
    except: Option<&str>,
) -> Result<Vec<PersonaPlan>> {
    let startable = startable(conn, run_id)?;
    let reserved: BTreeMap<String, String> = reservations(conn, run_id, except)?
        .into_iter()
        .filter(|(_, task)| startable.iter().any(|t| &t.task_id == task))
        .collect();
    let mut by: BTreeMap<String, (i64, Vec<String>)> = BTreeMap::new();
    for t in &startable {
        let Some(p) = &t.claimable_by else { continue };
        if reserved.values().any(|r| r == &t.task_id) {
            continue;
        }
        let entry = by.entry(p.clone()).or_default();
        entry.0 += 1;
        // A task whose last attempt died names that worker for --replaces.
        if let Some(last) = workers::attempts_at(conn, &t.task_id)?.last() {
            if last.status == WorkerStatus::Incomplete.as_str() {
                entry.1.push(last.worker_id.clone());
            }
        }
    }
    // A pre-claim row whose process went silent is not on its way (#190).
    let run = super::runs::get(conn, run_id)?;
    let now = util::now();
    let roster = workers::of_run(conn, run_id)?;
    let mut out = Vec::new();
    for (persona, (n, replaces)) in by {
        let on_the_way = roster
            .iter()
            .filter(|w| {
                w.kind == WorkerKind::Worker.as_str()
                    && !reserved.contains_key(&w.worker_id)
                    && w.persona.as_deref() == Some(persona.as_str())
                    && matches!(w.status.as_str(), "spawning" | "starting" | "claiming")
                    && !workers::is_stale(w, run.heartbeat_stale_s, &now)
            })
            .count() as i64;
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

/// Why the spawn of worker `worker_id` (a row in `run_id` with no task) is
/// refused, or `None`: the other workers of its persona still on their way
/// already cover the startable tasks of that persona (gap G-A).
pub fn spawn_refusal(conn: &Connection, run_id: &str, worker_id: &str) -> Result<Option<String>> {
    let w = workers::get(conn, worker_id)?;
    let Some(persona) = w.persona.as_deref() else {
        return Ok(None);
    };
    let plan = plan_workers_except(conn, run_id, Some(worker_id))?;
    let line = plan.iter().find(|p| p.persona == persona);
    let startable = line.map_or(0, |p| p.startable);
    // plan_workers counts this new row too, when the persona has a line and
    // the row is on its way.
    let mine = i64::from(matches!(
        w.status.as_str(),
        "spawning" | "starting" | "claiming"
    ));
    let others = line.map_or(0, |p| (p.on_the_way - mine).max(0));
    Ok((others >= startable).then(|| {
        format!(
            "persona {persona} has {startable} startable task(s), and {others} worker(s) \
             of it are already on their way. Run horch ledger plan-workers and spawn only \
             the count it prints."
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::workers::NewWorker;
    use crate::taskledger::Store;

    /// A store in `dir` with the run "r1".
    fn store_with_run(dir: &std::path::Path) -> (Store, Run) {
        let mut store = Store::open(&dir.join("ledger.sqlite3")).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| runs::create(tx, dir, &new_run))
            .unwrap();
        (store, run)
    }

    const TEXT: &str = "# 01: A\n\n<!-- horch-ticket: 1 -->\n**Persona:** primary `dev`\n";

    /// A new worker of `persona` in `run`, spawned for `target`.
    fn spawn_for(store: &mut Store, run: &Run, persona: &str, target: Option<&str>) -> String {
        let new = NewWorker {
            run_id: run.run_id.clone(),
            persona: persona.into(),
            target_task_id: target.map(str::to_string),
            ..Default::default()
        };
        store.immediate(|tx| workers::insert(tx, &new)).unwrap()
    }

    /// The task id `worker` claims, or `None` for no task.
    fn claim_id(store: &mut Store, worker: &str) -> Option<String> {
        match store.immediate(|tx| claim(tx, worker)).unwrap() {
            ClaimOutcome::Claimed(c) => Some(c.task.task_id),
            ClaimOutcome::NoTask { .. } => None,
        }
    }

    /// Review 31 I-08: an older startable task of `codex-reviewer` and then
    /// 1 code-sweep review. The worker spawned for the review claims the
    /// review, and no other worker takes it while that worker is live.
    #[test]
    fn trf_24_auto_spawned_reviewer_claims_its_review_not_an_older_task() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let add_task = |store: &mut Store, title: &str, kind: TaskKind| {
            let mut new = NewTask::work(title, &format!("tickets/{title}.md"), "codex-reviewer");
            new.kind = kind;
            store
                .immediate(|tx| add(tx, &run, &new, Actor::Orchestrator))
                .unwrap()
                .task_id
        };
        let older = add_task(&mut store, "older", TaskKind::Work);
        let review = add_task(&mut store, "review", TaskKind::Review);

        let reviewer = spawn_for(&mut store, &run, "codex-reviewer", Some(&review));
        // A plain worker claims first: it skips the reserved review.
        let plain = spawn_for(&mut store, &run, "codex-reviewer", None);
        assert_eq!(claim_id(&mut store, &plain), Some(older.clone()));
        let late = spawn_for(&mut store, &run, "codex-reviewer", None);
        assert_eq!(claim_id(&mut store, &late), None, "the review is reserved");
        assert_eq!(claim_id(&mut store, &reviewer), Some(review.clone()));

        // A reservation ends with its worker: the task is claimable again.
        let review2 = add_task(&mut store, "review2", TaskKind::Review);
        let dead = spawn_for(&mut store, &run, "codex-reviewer", Some(&review2));
        store
            .conn()
            .execute(
                "UPDATE workers SET status = ?2 WHERE worker_id = ?1",
                params![dead, WorkerStatus::NoTask.as_str()],
            )
            .unwrap();
        let next = spawn_for(&mut store, &run, "codex-reviewer", None);
        assert_eq!(claim_id(&mut store, &next), Some(review2));

        // A target that is no longer startable: the claim goes on as before.
        let other = add_task(&mut store, "other", TaskKind::Work);
        let missed = spawn_for(&mut store, &run, "codex-reviewer", Some(&review));
        assert_eq!(claim_id(&mut store, &missed), Some(other));

        // A target of another run is refused at spawn.
        let (mut store2, run2) = store_with_run(&dir.path().join("b"));
        let new = NewWorker {
            run_id: run2.run_id.clone(),
            persona: "codex-reviewer".into(),
            target_task_id: Some(older),
            ..Default::default()
        };
        assert!(store2.immediate(|tx| workers::insert(tx, &new)).is_err());
    }

    /// Report 47: a review task of `codex-reviewer` reserved by a live
    /// worker of another persona, and by a stale one. Each reserved task
    /// has its worker on the way: the plan asks for no extra worker.
    #[test]
    fn trf_51_a_reserved_task_needs_no_extra_worker() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let mut new = NewTask::work("review", "tickets/review.md", "codex-reviewer");
        new.kind = TaskKind::Review;
        let review = store
            .immediate(|tx| add(tx, &run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id;
        let spawn = |plan: &[PersonaPlan]| -> i64 { plan.iter().map(|p| p.spawn).sum() };
        assert_eq!(spawn(&plan_workers(store.conn(), &run.run_id).unwrap()), 1);

        // A worker of another persona was spawned for the review.
        let reviewer = spawn_for(&mut store, &run, "opus-reviewer", Some(&review));
        assert_eq!(
            spawn(&plan_workers(store.conn(), &run.run_id).unwrap()),
            0,
            "the review has its worker on the way"
        );
        // A worker of the persona of the review finds no task: it is reserved.
        let extra = spawn_for(&mut store, &run, "codex-reviewer", None);
        assert_eq!(claim_id(&mut store, &extra), None);

        // A stale reserving worker still holds the review: no spawn either.
        store
            .conn()
            .execute(
                "UPDATE workers SET status = 'claiming', last_checkin_at = '2000-01-01T00:00:00Z'
                 WHERE worker_id = ?1",
                [&reviewer],
            )
            .unwrap();
        assert_eq!(spawn(&plan_workers(store.conn(), &run.run_id).unwrap()), 0);

        // The reservation ends with its worker: the review needs 1 worker.
        store
            .conn()
            .execute(
                "UPDATE workers SET status = ?2 WHERE worker_id = ?1",
                params![reviewer, WorkerStatus::NoTask.as_str()],
            )
            .unwrap();
        assert_eq!(spawn(&plan_workers(store.conn(), &run.run_id).unwrap()), 1);
        // The gate of the spawned worker does not count its own reservation.
        let mine = spawn_for(&mut store, &run, "codex-reviewer", Some(&review));
        assert_eq!(
            spawn_refusal(store.conn(), &run.run_id, &mine).unwrap(),
            None
        );
    }

    #[test]
    fn trf_12_add_stores_the_snapshot_and_its_sha_and_a_claim_gives_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let mut new = NewTask::work("A", "tickets/a.md", "dev");
        new.ticket_snapshot = Some(TEXT.to_string());
        let t = store
            .immediate(|tx| add(tx, &run, &new, Actor::Orchestrator))
            .unwrap();
        assert_eq!(t.ticket_snapshot.as_deref(), Some(TEXT));
        assert_eq!(t.ticket_sha, Some(ticket_sha(TEXT)));
        assert_eq!(
            ticket_sha("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // The ticket file never existed: the claim needs only the ledger.
        let worker = NewWorker {
            run_id: run.run_id.clone(),
            persona: "dev".into(),
            ..Default::default()
        };
        let wid = store.immediate(|tx| workers::insert(tx, &worker)).unwrap();
        let ClaimOutcome::Claimed(c) = store.immediate(|tx| claim(tx, &wid)).unwrap() else {
            panic!("the worker claims the task");
        };
        assert_eq!(c.ticket_snapshot.as_deref(), Some(TEXT));
        // Status JSON leaves the snapshot out; the claim JSON has it.
        let task_json = serde_json::to_value(&c.task).unwrap();
        assert!(task_json.get("ticket_snapshot").is_none(), "{task_json}");
        assert_eq!(serde_json::to_value(&*c).unwrap()["ticket_snapshot"], TEXT);
    }

    #[test]
    fn trf_12_a_task_without_a_snapshot_stores_none() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let new = NewTask::work("A", "tickets/a.md", "dev");
        let t = store
            .immediate(|tx| add(tx, &run, &new, Actor::Orchestrator))
            .unwrap();
        assert_eq!((t.ticket_snapshot, t.ticket_sha), (None, None));
    }

    #[test]
    fn trf_13_a_ticket_already_queued_is_refused_and_a_cancelled_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let items = vec![
            ("1".to_string(), NewTask::work("A", "tickets/a.md", "dev")),
            ("2".to_string(), NewTask::work("B", "tickets/b.md", "dev")),
        ];
        store
            .immediate(|tx| refuse_queued(tx, &run.run_id, &items))
            .unwrap();
        let added = store
            .immediate(|tx| add_many(tx, &run, &items, Actor::Orchestrator))
            .unwrap();
        let e = store
            .immediate(|tx| refuse_queued(tx, &run.run_id, &items))
            .unwrap_err()
            .to_string();
        assert_eq!(
            e,
            "tickets/a.md is already T-1 (queued) in this run.\n\
             tickets/b.md is already T-2 (queued) in this run.\n\
             Nothing was queued. To queue these tickets again, add --again."
        );
        store.immediate(|tx| cancel(tx, &added[0], "test")).unwrap();
        let queued = store
            .immediate(|tx| already_queued(tx, &run.run_id, &["tickets/a.md", "tickets/b.md"]))
            .unwrap();
        assert_eq!(
            queued.iter().map(Task::short).collect::<Vec<_>>(),
            vec!["T-2"]
        );
    }

    #[test]
    fn trf_14_add_stores_kind_review_and_check_none() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let mut new = NewTask::work("A", "tickets/a.md", "dev");
        new.work_kind = Some(WorkKind::Refactor);
        new.review_mode = Some(ReviewMode::Full);
        new.check_none = true;
        let t = store
            .immediate(|tx| add(tx, &run, &new, Actor::Orchestrator))
            .unwrap();
        assert_eq!(t.work_kind.as_deref(), Some("refactor"));
        assert_eq!(t.review_mode.as_deref(), Some("full"));
        assert!(t.check_none);
        assert_eq!(t.land_check, None);
    }

    /// Queue a task of `persona` with `edit` applied.
    fn queue(
        store: &mut Store,
        run: &Run,
        title: &str,
        persona: &str,
        edit: impl Fn(&mut NewTask),
    ) -> Task {
        let mut new = NewTask::work(title, &format!("tickets/{title}.md"), persona);
        edit(&mut new);
        store
            .immediate(|tx| add(tx, run, &new, Actor::Orchestrator))
            .unwrap()
    }

    /// A new worker row of `persona`, in state `spawning`.
    fn hire(store: &mut Store, run: &Run, persona: &str) -> String {
        let new = NewWorker {
            run_id: run.run_id.clone(),
            persona: persona.into(),
            ..Default::default()
        };
        store.immediate(|tx| workers::insert(tx, &new)).unwrap()
    }

    #[test]
    fn trf_31_a_task_with_no_worker_of_its_persona_holds_no_slot() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        // x is first and exclusive, and owns the file y owns. No x worker exists.
        queue(&mut store, &run, "x", "x", |n| {
            n.parallel = false;
            n.priority = 1;
            n.own = vec!["src/a.rs".into()];
        });
        queue(&mut store, &run, "y", "y", |n| {
            n.own = vec!["src/a.rs".into()]
        });
        let start: Vec<String> = startable(store.conn(), &run.run_id)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(
            start,
            vec!["x", "y"],
            "both show, so plan-workers asks for both"
        );
        let wy = hire(&mut store, &run, "y");
        let ClaimOutcome::Claimed(c) = store.immediate(|tx| claim(tx, &wy)).unwrap() else {
            panic!("the y worker claims y: x holds no slot");
        };
        assert_eq!(c.task.title, "y");
    }

    #[test]
    fn trf_31_a_task_with_a_starting_worker_of_its_persona_holds_its_slot() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        queue(&mut store, &run, "x", "x", |n| {
            n.parallel = false;
            n.priority = 1;
        });
        queue(&mut store, &run, "y", "y", |_| {});
        hire(&mut store, &run, "x");
        let wy = hire(&mut store, &run, "y");
        let waits = waits(store.conn(), &run.run_id).unwrap();
        assert_eq!(waits.len(), 1, "{waits:?}");
        assert_eq!(waits[0].0.title, "y");
        assert!(matches!(
            store.immediate(|tx| claim(tx, &wy)).unwrap(),
            ClaimOutcome::NoTask { .. }
        ));
    }

    #[test]
    fn trf_31_a_spawn_is_refused_when_workers_on_their_way_cover_the_plan() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        queue(&mut store, &run, "a", "dev", |_| {});
        let first = hire(&mut store, &run, "dev");
        assert_eq!(
            spawn_refusal(store.conn(), &run.run_id, &first).unwrap(),
            None,
            "the first worker covers the 1 startable task"
        );
        let second = hire(&mut store, &run, "dev");
        let why = spawn_refusal(store.conn(), &run.run_id, &second)
            .unwrap()
            .expect("a second dev worker is over the plan");
        assert!(why.contains("horch ledger plan-workers"), "{why}");
        let other = hire(&mut store, &run, "idle");
        assert!(
            spawn_refusal(store.conn(), &run.run_id, &other)
                .unwrap()
                .is_some(),
            "a persona with no startable task gets no worker"
        );
    }

    #[test]
    fn trf_32_a_claim_with_no_task_ends_the_worker_and_tells_the_orchestrator() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let w = hire(&mut store, &run, "dev");
        assert!(matches!(
            store.immediate(|tx| claim(tx, &w)).unwrap(),
            ClaimOutcome::NoTask { .. }
        ));
        let worker = workers::get(store.conn(), &w).unwrap();
        assert_eq!(worker.status, WorkerStatus::NoTask.as_str());
        assert!(worker.ended_at.is_some(), "the worker ended");
        let (event, key, text, delivered): (String, String, String, Option<String>) = store
            .conn()
            .query_row(
                "SELECT event, key, text, delivered_at FROM outbox WHERE run_id = ?1",
                [&run.run_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(event, "no-task");
        assert_eq!(key, format!("{}:no-task:{w}", run.run_id));
        assert!(text.contains("found no task for persona dev"), "{text}");
        assert_eq!(delivered, None);
    }

    /// A run with tasks "a" (priority 100) and "b" (priority 50) for dev.
    /// A dev worker claims b, and b gets an open worktree. `dir` holds a
    /// handoff file. Returns the worker id, task b, and the handoff path.
    fn handing_off(store: &mut Store, run: &Run, dir: &std::path::Path) -> (String, Task, String) {
        queue(store, run, "a", "dev", |_| {});
        queue(store, run, "b", "dev", |n| n.priority = 50);
        let w = hire(store, run, "dev");
        let ClaimOutcome::Claimed(c) = store.immediate(|tx| claim(tx, &w)).unwrap() else {
            panic!("the worker claims b");
        };
        assert_eq!(c.task.title, "b");
        store
            .immediate(|tx| {
                tx.execute(
                    "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path,
                        branch, base, status, origin, created_at)
                     VALUES ('wt1', ?1, ?2, '/repo', '/repo/.worktrees/b', 'fleet/b', 'main',
                        'open', 'native', ?3)",
                    params![c.task.task_id, w, util::now()],
                )?;
                Ok(())
            })
            .unwrap();
        let path = dir.join("opus-1-whats-next.md");
        std::fs::write(&path, "# Handoff\n").unwrap();
        (w, c.task.clone(), path.to_string_lossy().into_owned())
    }

    #[test]
    fn trf_37_handoff_ends_the_worker_and_requeues_the_task_first_with_its_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let (w, b, path) = handing_off(&mut store, &run, dir.path());
        let out = store.immediate(|tx| handoff(tx, &w, &path)).unwrap();
        // The worker ended handed off, with the path.
        assert_eq!(out.worker.status, WorkerStatus::Incomplete.as_str());
        assert_eq!(out.worker.end_reason.as_deref(), Some("handed_off"));
        assert_eq!(out.worker.handoff_path.as_deref(), Some(path.as_str()));
        assert!(out.worker.ended_at.is_some());
        // The task is queued, in front of a, and its worktree is open.
        assert_eq!(out.task.status, TaskStatus::Queued.as_str());
        assert_eq!(out.task.current_worker_id, None);
        let a = resolve(store.conn(), &run.run_id, "T-1").unwrap();
        assert!(
            out.task.priority < a.priority,
            "{} < {}",
            out.task.priority,
            a.priority
        );
        let (status, holder): (String, Option<String>) = store
            .conn()
            .query_row(
                "SELECT status, worker_id FROM worktrees WHERE worktree_id = 'wt1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((status.as_str(), holder), ("open", None));
        // A startable wake, in the same transaction, names the replacement.
        let (event, text): (String, String) = store
            .conn()
            .query_row(
                "SELECT event, text FROM outbox WHERE id = ?1",
                [out.wake_id.as_deref().expect("a new wake row")],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(event, "startable");
        assert!(
            text.contains(&format!("handed off {}", b.short())),
            "{text}"
        );
        assert!(text.contains("Spawn: dev x1"), "{text}");
        // plan-workers names the handed-off worker for --replaces.
        let plan = plan_workers(store.conn(), &run.run_id).unwrap();
        assert_eq!(plan[0].replaces, vec![w.clone()]);
    }

    #[test]
    fn trf_37_the_next_claim_takes_the_task_its_worktree_and_the_handoff_path() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let (w, b, path) = handing_off(&mut store, &run, dir.path());
        store.immediate(|tx| handoff(tx, &w, &path)).unwrap();
        let next = hire(&mut store, &run, "dev");
        let ClaimOutcome::Claimed(c) = store.immediate(|tx| claim(tx, &next)).unwrap() else {
            panic!("the replacement claims");
        };
        assert_eq!(c.task.task_id, b.task_id, "the handed-off task comes first");
        assert_eq!(c.handoff_path.as_deref(), Some(path.as_str()));
        assert_eq!(c.worktree.as_deref(), Some("/repo/.worktrees/b"));
        assert_eq!(c.attempt, 2);
    }

    #[test]
    fn trf_37_a_claim_after_an_attempt_that_did_not_hand_off_has_no_handoff_path() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let (w, _, _) = handing_off(&mut store, &run, dir.path());
        store
            .immediate(|tx| {
                workers::end(tx, &w, WorkerStatus::Incomplete, "reaped", None)?;
                requeue(tx, &resolve(tx, &run.run_id, "T-2")?.task_id, "reaped")
            })
            .unwrap();
        let next = hire(&mut store, &run, "dev");
        let ClaimOutcome::Claimed(c) = store.immediate(|tx| claim(tx, &next)).unwrap() else {
            panic!("the replacement claims");
        };
        assert_eq!(c.handoff_path, None);
    }

    #[test]
    fn trf_37_handoff_refuses_a_missing_file_and_a_worker_with_no_running_task() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let (w, _, path) = handing_off(&mut store, &run, dir.path());
        let missing = dir.path().join("nope.md");
        let err = store
            .immediate(|tx| handoff(tx, &w, &missing.to_string_lossy()))
            .unwrap_err();
        assert!(format!("{err:#}").contains("does not exist"), "{err:#}");
        // Nothing changed: the worker still works.
        let worker = workers::get(store.conn(), &w).unwrap();
        assert_eq!(worker.status, WorkerStatus::Working.as_str());
        // A worker with no task cannot hand off.
        let idle = hire(&mut store, &run, "dev");
        let err = store.immediate(|tx| handoff(tx, &idle, &path)).unwrap_err();
        assert!(format!("{err:#}").contains("holds no task"), "{err:#}");
        // A second handoff of the same worker is refused.
        store.immediate(|tx| handoff(tx, &w, &path)).unwrap();
        assert!(store.immediate(|tx| handoff(tx, &w, &path)).is_err());
    }
}
