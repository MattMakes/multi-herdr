use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Row, Transaction};
use serde::Serialize;

use super::model::{
    Actor, EventKind, FileMode, FindingStatus, HeldReason, Origin, OutboxEvent, ReviewMode,
    TaskKind, TaskStatus, WorkKind, WorkerKind, WorkerStatus, WorktreeMode, WorktreeStatus,
};
use super::outbox::{self, Wake};
use super::runs::Run;
use super::sql_words::{self, word};
use super::store::optional;
use super::util::{self, About};
use super::workers::{self, Worker};
use crate::fleetd::fleet_runtime::adapters::clock_store;
use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::task_execution::application::retry::Policy;
use crate::fleetd::task_execution::domain::retry::{self, claimable_at, rung6, Numbers, Rung6};
use crate::fleetd::worktree_custody::adapters::sqlite::{self as trees, WorktreeRow};
use crate::fleetd::worktree_custody::application::create;

pub const RETRY_PRIORITY_BOOST: i64 = 1000;

pub const ONE_OFF_TICKET: &str = "(one-off: no ticket file)";

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
    #[serde(flatten, serialize_with = "status_json")]
    pub status: String,
    pub attempts: i64,
    pub held_reason: Option<String>,
    pub attempt_floor: i64,
    pub not_before: Option<i64>,
    pub due_at: Option<i64>,
    pub replans: i64,
    pub current_worker_id: Option<String>,
    pub summary: Option<String>,
    pub origin: String,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub work_kind: Option<String>,
    pub review_mode: Option<String>,
    #[serde(skip_serializing)]
    pub ticket_snapshot: Option<String>,
    pub ticket_sha: Option<String>,
    pub check_none: bool,
}

impl Task {
    pub fn short(&self) -> String {
        format!("T-{}", self.seq)
    }

    pub fn status(&self) -> Result<TaskStatus> {
        TaskStatus::parse(&self.status)
    }

    pub fn shown_status(&self) -> &str {
        TaskStatus::shown_word(&self.status)
    }

    pub fn is_queued(&self) -> bool {
        is_in(sql_words::TASKS_QUEUED, &self.status)
    }

    pub fn in_flight(&self) -> bool {
        is_in(sql_words::TASKS_IN_FLIGHT, &self.status)
    }

    pub fn is_review(&self) -> bool {
        self.kind == TaskKind::Review.as_str()
    }

    pub fn uses_worktree(&self) -> bool {
        self.worktree_mode == WorktreeMode::Worktree.as_str()
    }

    pub fn is_no_change_one_off(&self) -> bool {
        self.check_none && self.task_file.as_deref() == Some(ONE_OFF_TICKET)
    }

    pub fn is_parked(&self) -> bool {
        parked(&self.status, self.held_reason.as_deref())
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
            not_before: r.get("not_before")?,
            due_at: r.get("due_at")?,
            replans: r.get("replans")?,
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
    pub blocked_by: Vec<String>,
    pub own: Vec<String>,
    pub no_touch: Vec<String>,
    pub work_kind: Option<WorkKind>,
    pub review_mode: Option<ReviewMode>,
    pub ticket_snapshot: Option<String>,
    pub check_none: bool,
}

impl NewTask {
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

fn status_json<S: serde::Serializer>(status: &str, s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = s.serialize_map(Some(2))?;
    map.serialize_entry("status", TaskStatus::shown_word(status))?;
    map.serialize_entry("state", status)?;
    map.end()
}

fn is_in(states: &[TaskStatus], status: &str) -> bool {
    states.iter().any(|s| s.as_str() == status)
}

fn first_state(all_done: bool) -> TaskStatus {
    if all_done {
        TaskStatus::Ready
    } else {
        TaskStatus::Pending
    }
}

pub fn ticket_sha(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn already_queued(conn: &Connection, run_id: &str, files: &[&str]) -> Result<Vec<Task>> {
    Ok(of_run(conn, run_id)?
        .into_iter()
        .filter(|t| t.status != TaskStatus::Cancelled.as_str())
        .filter(|t| t.task_file.as_deref().is_some_and(|f| files.contains(&f)))
        .collect())
}

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
                t.shown_status()
            )
        })
        .collect();
    bail!(
        "{}\nNothing was queued. To queue these tickets again, add --again.",
        lines.join("\n")
    )
}

pub fn parked(status: &str, held_reason: Option<&str>) -> bool {
    status == TaskStatus::Escalated.as_str() && held_reason == Some(HeldReason::Unlanded.as_str())
}

pub fn get(conn: &Connection, task_id: &str) -> Result<Task> {
    optional(conn.query_row(
        "SELECT * FROM tasks WHERE task_id = ?1",
        [task_id],
        Task::from_row,
    ))?
    .with_context(|| format!("no task {task_id}"))
}

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

pub fn of_run(conn: &Connection, run_id: &str) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare("SELECT * FROM tasks WHERE run_id = ?1 ORDER BY seq")?;
    let rows = stmt
        .query_map([run_id], Task::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

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

pub fn unblock(conn: &Connection, task: &Task, blocker: &Task) -> Result<()> {
    util::atomic(conn, |conn| unblock_in(conn, task, blocker))
}

fn unblock_in(conn: &Connection, task: &Task, blocker: &Task) -> Result<()> {
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
    ready_when_met(conn, &task.task_id)
}

pub fn ready_dependents(conn: &Connection, task_id: &str) -> Result<()> {
    let waiting: Vec<String> = conn
        .prepare("SELECT task_id FROM task_deps WHERE blocked_by = ?1 ORDER BY task_id")?
        .query_map([task_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in waiting {
        ready_when_met(conn, &id)?;
    }
    Ok(())
}

fn ready_when_met(conn: &Connection, task_id: &str) -> Result<()> {
    let task = get(conn, task_id)?;
    let met = blockers(conn, task_id)?
        .iter()
        .all(|b| b.status == TaskStatus::Done.as_str());
    if task.status != TaskStatus::Pending.as_str() || !met {
        return Ok(());
    }
    let text = format!("each task that {} waits for is done", task.short());
    util::transition(
        conn,
        Entity::Task,
        task_id,
        TaskStatus::Ready.as_str(),
        Actor::Horch,
        util::Why::Rule(util::Rule::DependenciesMet, &text),
    )
}

pub fn files(conn: &Connection, task_id: &str, mode: FileMode) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT path FROM task_files WHERE task_id = ?1 AND mode = ?2 ORDER BY path")?;
    let rows = stmt
        .query_map(params![task_id, mode.as_str()], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(rows)
}

pub fn files_of_tasks_in(
    conn: &Connection,
    statuses: &[TaskStatus],
    mode: FileMode,
) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let sql = format!(
        "SELECT f.task_id, f.path FROM task_files f JOIN tasks t ON t.task_id = f.task_id
         WHERE t.status IN ({}) AND f.mode = ?1 ORDER BY f.task_id, f.path",
        super::sql_words::list(statuses)
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([mode.as_str()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for row in rows {
        let (task_id, path) = row?;
        out.entry(task_id).or_default().push(path);
    }
    Ok(out)
}

pub fn add(conn: &Connection, run: &Run, new: &NewTask, actor: Actor) -> Result<Task> {
    validate(new)?;
    let blockers: Vec<Task> = new
        .blocked_by
        .iter()
        .map(|b| resolve(conn, &run.run_id, b))
        .collect::<Result<_>>()?;
    let state = first_state(
        blockers
            .iter()
            .all(|b| b.status == TaskStatus::Done.as_str()),
    );
    let id = insert(conn, run, new, state)?;
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
        let blockers: Vec<(String, bool)> = new
            .blocked_by
            .iter()
            .map(|b| match keys.get(b.as_str()) {
                Some(&j) => Ok((ids[j].clone().expect("blockers insert first"), false)),
                None => resolve(conn, &run.run_id, b)
                    .map(|t| (t.task_id, t.status == TaskStatus::Done.as_str())),
            })
            .collect::<Result<_>>()?;
        let id = insert(
            conn,
            run,
            new,
            first_state(blockers.iter().all(|(_, done)| *done)),
        )?;
        for (blocker, _) in blockers {
            conn.execute(
                "INSERT INTO task_deps (task_id, blocked_by) VALUES (?1, ?2)",
                params![id, blocker],
            )?;
        }
        ids[i] = Some(id);
    }
    Ok(ids)
}

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

fn idea_of_new_task(conn: &Connection, run: &Run, new: &NewTask) -> Result<String> {
    if let Some(parent) = &new.parent_task_id {
        let idea: Option<String> = conn
            .query_row(
                "SELECT idea_id FROM tasks WHERE task_id = ?1",
                [parent],
                |r| r.get(0),
            )
            .with_context(|| format!("the parent task {parent} is not in the ledger"))?;
        return idea.with_context(|| {
            format!("the parent task {parent} has no idea, and a task needs an idea (FR-12)")
        });
    }
    let rowid: i64 = conn.query_row(
        "SELECT rowid FROM runs WHERE run_id = ?1",
        [&run.run_id],
        |r| r.get(0),
    )?;
    let feature = format!("F-{rowid:04}");
    crate::fleetd::integration::application::stacks::youngest_idea_in_flight(conn, &feature)?
        .with_context(|| {
            format!(
                "run {} has no idea in flight of its feature {feature}, and a task needs an idea (FR-12). Add one: horch idea add",
                run.slug
            )
        })
}

fn insert(conn: &Connection, run: &Run, new: &NewTask, state: TaskStatus) -> Result<String> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM tasks WHERE run_id = ?1",
        [&run.run_id],
        |r| r.get(0),
    )?;
    let id = util::new_id();
    let now = util::now();
    let idea = idea_of_new_task(conn, run, new)?;
    conn.execute(
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, issue_url, spec_ref,
            primary_persona, fallback_persona, claimable_by, phase, parallel, priority, wave,
            worktree_mode, land_check, kind, parent_task_id, status, origin, created_at, updated_at,
            work_kind, review_mode, ticket_snapshot, ticket_sha, check_none, idea_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
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
            state.as_str(),
            Origin::Native.as_str(),
            now,
            new.work_kind.map(WorkKind::as_str),
            new.review_mode.map(ReviewMode::as_str),
            new.ticket_snapshot,
            new.ticket_snapshot.as_deref().map(ticket_sha),
            new.check_none as i64,
            idea,
        ],
    )?;
    for (paths, mode) in [
        (&new.own, FileMode::Own),
        (&new.no_touch, FileMode::DoNotTouch),
    ] {
        add_files(conn, &id, paths, mode)?;
    }
    Ok(id)
}

pub fn add_files(conn: &Connection, task_id: &str, paths: &[String], mode: FileMode) -> Result<()> {
    for p in paths {
        conn.execute(
            "INSERT OR IGNORE INTO task_files (task_id, path, mode) VALUES (?1, ?2, ?3)",
            params![task_id, p.trim(), mode.as_str()],
        )?;
    }
    Ok(())
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Wait {
    BlockedBy { task: String, status: String },
    BlockerCancelled { task: String },
    BlockerParked { task: String },
    Exclusive { running: String },
    FileHeld { path: String, by: String },
    SharedCheckout { running: String },
    Backoff { seconds: i64 },
}

impl std::fmt::Display for Wait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Wait::BlockedBy { task, status } => write!(f, "blocked by {task} ({status})"),
            Wait::BlockerCancelled { task } => {
                write!(
                    f,
                    "{task} was cancelled and cannot run again: drop the dependency, or cancel this task. A new task from its ticket does not end this wait"
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
            Wait::Backoff { seconds } => write!(f, "retry after {seconds} s"),
        }
    }
}

fn pairs(conn: &Connection, sql: &str, run_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([run_id], |r| r.try_into())?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn exclusive(t: &Task, work: &[&&Task]) -> Option<Wait> {
    let blocker = work
        .iter()
        .find(|a| !a.parallel)
        .or_else(|| (!t.parallel).then(|| work.first()).flatten());
    blocker.map(|x| Wait::Exclusive { running: x.short() })
}

fn shared_checkout(t: &Task, work: &[&&Task]) -> Option<Wait> {
    if t.uses_worktree() {
        return None;
    }
    work.iter()
        .find(|a| !a.uses_worktree())
        .map(|a| Wait::SharedCheckout { running: a.short() })
}

struct Board {
    tasks: Vec<Task>,
    own: BTreeMap<String, Vec<String>>,
    deps: BTreeMap<String, Vec<String>>,
    staffed: BTreeSet<String>,
    clock: Option<FleetSeconds>,
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
            clock: fleet_clock_now(conn)?,
        })
    }

    fn is_staffed(&self, t: &Task) -> bool {
        t.claimable_by
            .as_deref()
            .is_some_and(|p| self.staffed.contains(p))
    }

    fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.task_id == id)
    }

    fn is_active(t: &Task) -> bool {
        t.in_flight()
    }

    fn wait(&self, t: &Task, active: &[&Task]) -> Option<Wait> {
        self.backoff(t).or_else(|| self.blocked_by(t)).or_else(|| {
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

    fn backoff(&self, t: &Task) -> Option<Wait> {
        let clock = self.clock?;
        let not_before = t.not_before?;
        (!claimable_at(Some(not_before), clock.0)).then(|| Wait::Backoff {
            seconds: not_before - clock.0,
        })
    }

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
                        status: bt.shown_status().to_string(),
                    })
                } else {
                    None
                }
            })
    }

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

    fn queued(&self) -> Vec<&Task> {
        let mut q: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| is_in(sql_words::TASKS_WAITING, &t.status) && t.held_reason.is_none())
            .collect();
        q.sort_by_key(|t| (t.priority, t.seq));
        q
    }

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

    fn startable(&self) -> Vec<&Task> {
        self.plan()
            .into_iter()
            .filter(|(_, w)| w.is_none())
            .map(|(t, _)| t)
            .collect()
    }
}

pub fn startable(conn: &Connection, run_id: &str) -> Result<Vec<Task>> {
    let board = Board::load(conn, run_id)?;
    Ok(board.startable().into_iter().cloned().collect())
}

pub fn waits(conn: &Connection, run_id: &str) -> Result<Vec<(Task, Wait)>> {
    let board = Board::load(conn, run_id)?;
    Ok(board
        .plan()
        .into_iter()
        .filter_map(|(t, w)| w.map(|w| (t.clone(), w)))
        .collect())
}

pub const PUSH_FINISH_REFUSAL: &str =
    "run `horch result` when the work is committed; fleetd runs the finish steps";

pub fn push_attempt_of(conn: &Connection, worker_id: &str) -> Result<Option<(String, i64)>> {
    let row: Option<(Option<String>, Option<i64>)> = optional(conn.query_row(
        "SELECT a.task_id, w.attempt FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.worker_id = ?1 AND a.via = 'push' ORDER BY a.rowid DESC LIMIT 1",
        [worker_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    Ok(match row {
        Some((Some(task), Some(n))) => Some((task, n)),
        _ => None,
    })
}

pub fn cancel_requested(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let reason = conn.query_row(
        "SELECT cancel_requested FROM tasks WHERE task_id = ?1",
        [task_id],
        |r| r.get(0),
    )?;
    Ok(reason)
}

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

fn held_tree(conn: &Connection, task_id: &str) -> Result<Option<WorktreeRow>> {
    Ok(trees::of_task(conn, task_id)?.filter(|r| sql_words::WORKTREES_HELD.contains(&r.status)))
}

#[derive(Debug, Clone, PartialEq)]
pub struct HandedOff {
    pub task: Task,
    pub worker: Worker,
    pub wake_id: Option<String>,
}

pub fn handoff(tx: &Transaction, worker_id: &str, path: &str) -> Result<HandedOff> {
    let w = workers::get(tx, worker_id)?;
    let task = handoff_task(tx, &w)?;
    if !std::path::Path::new(path).is_file() {
        bail!("the handoff file {path} does not exist; write it first");
    }
    workers::hand_off(tx, worker_id, path)?;
    util::transition(
        tx,
        Entity::Task,
        &task.task_id,
        TaskStatus::Ready.as_str(),
        Actor::Worker,
        util::Why::Event(EventKind::Requeued, &format!("{} handed off {}: it is queued again, first in line, with its worktree. Handoff: {path}", w.label(), task.short())),
    )?;
    tx.execute(
        &format!(
            "UPDATE tasks SET current_worker_id = NULL, held_reason = NULL,
                priority = MIN(priority, (SELECT COALESCE(MIN(o.priority), tasks.priority)
                    FROM tasks o WHERE o.run_id = tasks.run_id AND o.status IN ({queued})
                    AND o.task_id != tasks.task_id) - 1),
                updated_at = ?2
             WHERE task_id = ?1",
            queued = sql_words::list(sql_words::TASKS_QUEUED)
        ),
        params![task.task_id, util::now()],
    )?;
    if let Some(row) = held_tree(tx, &task.task_id)? {
        create::release(tx, &row, "its attempt handed off the task")?;
    }
    let wake_id = outbox::write(tx, &task.run_id, &handoff_wake(&w, &task, path))?;
    Ok(HandedOff {
        task: get(tx, &task.task_id)?,
        worker: workers::get(tx, worker_id)?,
        wake_id,
    })
}

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
            task.shown_status(),
            w.label(),
            w.status
        );
    }
    Ok(task)
}

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

pub fn fallback(conn: &Connection, task: &Task, reason: &str) -> Result<Task> {
    let Some(fb) = &task.fallback_persona else {
        bail!("{} has no fallback persona", task.short());
    };
    if !task.is_queued() {
        bail!(
            "{} is {}; only a queued task moves to its fallback",
            task.short(),
            task.shown_status()
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

pub fn cancel(conn: &Connection, task: &Task, reason: &str) -> Result<Task> {
    if is_in(sql_words::TASKS_ENDED, &task.status) {
        bail!("{} is already {}", task.short(), task.shown_status());
    }
    util::atomic(conn, |conn| cancel_in(conn, task, reason))
}

fn cancel_in(conn: &Connection, task: &Task, reason: &str) -> Result<Task> {
    let why = format!("task cancelled: {reason}");
    util::transition(
        conn,
        Entity::Task,
        &task.task_id,
        TaskStatus::Cancelled.as_str(),
        Actor::Orchestrator,
        util::Why::Event(
            EventKind::Cancelled,
            &format!("{} cancelled: {reason}", task.short()),
        ),
    )?;
    conn.execute(
        "UPDATE tasks SET summary = COALESCE(summary, ?2), updated_at = ?3,
            finished_at = ?3, current_worker_id = NULL, held_reason = NULL
         WHERE task_id = ?1",
        params![task.task_id, format!("cancelled: {reason}"), util::now()],
    )?;
    needs_you::resolve(conn, &escalated_key(&task.task_id))?;
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
    get(conn, &task.task_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requeue {
    Skipped,
    Queued,
    Failed,
    Escalated,
}

pub const TASK_ESCALATED: &str = "task_escalated";

pub fn escalated_key(task_id: &str) -> String {
    format!("{TASK_ESCALATED}:{task_id}")
}

pub fn requeue(conn: &Connection, task_id: &str, why: &str) -> Result<Requeue> {
    let task = get(conn, task_id)?;
    if !task.in_flight() {
        return Ok(Requeue::Skipped);
    }
    util::atomic(conn, |conn| rung6_in(conn, &task, why, &Policy::default()))
}

pub fn burn_attempt(
    conn: &Connection,
    task_id: &str,
    why: &str,
    policy: &Policy,
) -> Result<Requeue> {
    let task = get(conn, task_id)?;
    let ending = [
        TaskStatus::Leased,
        TaskStatus::Running,
        TaskStatus::Stalled,
        TaskStatus::Verifying,
    ];
    if !is_in(&ending, &task.status) {
        bail!(
            "{} is {}; rung 6 applies only to a task whose attempt ended",
            task.short(),
            task.status
        );
    }
    rung6_in(conn, &task, why, policy)
}

fn rung6_in(conn: &Connection, task: &Task, why: &str, policy: &Policy) -> Result<Requeue> {
    if cancel_requested(conn, &task.task_id)?.is_some() {
        return Ok(Requeue::Skipped);
    }
    let run_max = super::runs::get(conn, &task.run_id)?.max_attempts;
    let n = Numbers {
        used: task.attempts - task.attempt_floor,
        limit: retry::limit(policy.retry.max_attempts, run_max),
        replans: task.replans,
    };
    let clock = fleet_clock_now(conn)?;
    let at = clock.map_or(0, |c| c.0);
    match rung6(&n, &policy.retry.backoff_s, policy.replan_limit_s, at) {
        Rung6::Retry { not_before } => {
            retry_in(conn, task, why, clock.map(|_| FleetSeconds(not_before)))?;
            Ok(Requeue::Queued)
        }
        Rung6::Fail { due_at } => {
            fail_in(conn, task, why, clock.map(|_| FleetSeconds(due_at)))?;
            outbox::write(conn, &task.run_id, &replan_wake(conn, task, policy)?)?;
            Ok(Requeue::Failed)
        }
        Rung6::Escalate => {
            fail_in(conn, task, why, None)?;
            hold_in(conn, task)?;
            Ok(Requeue::Escalated)
        }
    }
}

fn fleet_clock_now(conn: &Connection) -> Result<Option<FleetSeconds>> {
    if door::generation(conn)?.is_none() {
        return Ok(None);
    }
    Ok(Some(clock_store::read(conn)?))
}

fn replan_wake(conn: &Connection, task: &Task, policy: &Policy) -> Result<Wake> {
    let t = task.short();
    let used = task.attempts - task.attempt_floor;
    let text = format!(
        "QUESTION: replan {t}: requeue it 1 time, or cancel it. {used} attempts failed ({}). \
         Requeue: horch ledger requeue {t} \"<reason>\". Cancel: horch ledger cancel {t} \"<reason>\". \
         After {} s of the fleet clock with neither, {t} goes to the operator.",
        attempt_ends(conn, task)?.join("; "),
        policy.replan_limit_s
    );
    Ok(Wake::new(
        OutboxEvent::Held,
        format!("replan:{}:{}", task.task_id, task.attempts),
        super::wakes::tagged(&text),
    ))
}

pub fn failed(conn: &Connection) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT * FROM tasks WHERE status = {failed} ORDER BY task_id",
        failed = word(TaskStatus::Failed)
    ))?;
    let rows = stmt
        .query_map([], Task::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn set_replan_due(
    conn: &Connection,
    task_id: &str,
    clock: FleetSeconds,
    limit_s: u64,
) -> Result<()> {
    let limit = i64::try_from(limit_s).unwrap_or(i64::MAX);
    conn.execute(
        "UPDATE tasks SET due_at = ?2, updated_at = ?3 WHERE task_id = ?1",
        params![task_id, clock.0.saturating_add(limit), util::now()],
    )?;
    Ok(())
}

pub fn escalate(conn: &Connection, task_id: &str) -> Result<()> {
    let task = get(conn, task_id)?;
    if task.status != TaskStatus::Failed.as_str() {
        fail_in(conn, &task, "escalated by fleetd", None)?;
    }
    hold_in(conn, &task)
}

pub fn task_facts(
    conn: &Connection,
    task_id: &str,
    kind: EventKind,
) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT data_json FROM events WHERE task_id = ?1 AND kind = ?2 AND entity = ?3
           AND data_json IS NOT NULL
         ORDER BY event_id",
    )?;
    let rows = stmt.query_map(
        params![task_id, kind.as_str(), Entity::Task.as_str()],
        |r| r.get::<_, String>(0),
    )?;
    rows.map(|raw| Ok(serde_json::from_str(&raw?)?)).collect()
}

pub fn task_facts_of<'a>(
    conn: &Connection,
    task_ids: impl IntoIterator<Item = &'a str>,
    kind: EventKind,
) -> Result<Vec<(String, serde_json::Value)>> {
    let ids: Vec<&str> = task_ids.into_iter().collect();
    let sql = format!(
        "SELECT task_id, data_json FROM events WHERE kind = ?1 AND entity = ?2
           AND data_json IS NOT NULL AND task_id IN ({})
         ORDER BY event_id",
        crate::taskledger::sql_words::marks(3, ids.len())
    );
    let mut values = vec![kind.as_str(), Entity::Task.as_str()];
    values.extend(ids);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (task_id, raw) = row?;
        Ok((task_id, serde_json::from_str(&raw)?))
    })
    .collect()
}

pub fn question_answered(conn: &Connection, task_id: &str, question_id: &str) -> Result<bool> {
    if get(conn, task_id)?.status != TaskStatus::BlockedOnUser.as_str() {
        return Ok(false);
    }
    let text = format!("question_answered: {question_id}");
    let cause = crate::fleetd::fleet_runtime::application::ledger::Cause {
        actor: Actor::Horch,
        text: &text,
    };
    crate::fleetd::fleet_runtime::application::ledger::transition(
        conn,
        None,
        Entity::Task,
        task_id,
        TaskStatus::Ready.as_str(),
        cause,
        &needs_you::NeedsYouList,
    )?;
    conn.execute(
        "UPDATE tasks SET current_worker_id = NULL, due_at = NULL, updated_at = ?2
         WHERE task_id = ?1",
        params![task_id, util::now()],
    )?;
    Ok(true)
}

fn retry_in(
    conn: &Connection,
    task: &Task,
    why: &str,
    not_before: Option<FleetSeconds>,
) -> Result<()> {
    let task_id = task.task_id.as_str();
    let ready = TaskStatus::Ready.as_str();
    let after = match not_before {
        Some(t) => format!(", not before fleet clock {} s", t.0),
        None => String::new(),
    };
    let text = format!(
        "{} is queued again, first in line{after}: {why}",
        task.short()
    );
    let cause = util::Why::Event(EventKind::Requeued, &text);
    util::transition(conn, Entity::Task, task_id, ready, Actor::Horch, cause)?;
    conn.execute(
        "UPDATE tasks SET current_worker_id = NULL, priority = priority - ?2, not_before = ?4,
            due_at = NULL, updated_at = ?3
         WHERE task_id = ?1",
        params![
            task_id,
            RETRY_PRIORITY_BOOST,
            util::now(),
            not_before.map(|t| t.0)
        ],
    )?;
    Ok(())
}

fn fail_in(conn: &Connection, task: &Task, why: &str, due_at: Option<FleetSeconds>) -> Result<()> {
    let task_id = task.task_id.as_str();
    let used = task.attempts - task.attempt_floor;
    let text = format!(
        "{} failed: {used} attempts failed, the last: {why}",
        task.short()
    );
    let cause = util::Why::Event(EventKind::Blocked, &text);
    let failed = TaskStatus::Failed.as_str();
    util::transition(conn, Entity::Task, task_id, failed, Actor::Horch, cause)?;
    conn.execute(
        "UPDATE tasks SET current_worker_id = NULL, not_before = NULL, due_at = ?2, updated_at = ?3
         WHERE task_id = ?1",
        params![task_id, due_at.map(|t| t.0), util::now()],
    )?;
    Ok(())
}

fn hold_in(conn: &Connection, task: &Task) -> Result<()> {
    let task_id = task.task_id.as_str();
    let escalated = TaskStatus::Escalated.as_str();
    let used = task.attempts - task.attempt_floor;
    let held = format!("{used} attempts failed");
    let text = format!("{} is held for the operator: {held} ({}). Release it with horch ledger requeue {} \"<reason>\".", task.short(), attempt_ends(conn, task)?.join("; "), task.short());
    let cause = util::Why::Event(EventKind::Blocked, &text);
    util::transition(conn, Entity::Task, task_id, escalated, Actor::Horch, cause)?;
    conn.execute(
        "UPDATE tasks SET current_worker_id = NULL, held_reason = ?2, not_before = NULL,
            due_at = NULL, updated_at = ?3
         WHERE task_id = ?1",
        params![task_id, held, util::now()],
    )?;
    let key = escalated_key(task_id);
    needs_you::open_item(
        conn,
        &Write {
            key: &key,
            kind: TASK_ESCALATED,
            entity: Entity::Task.as_str(),
            entity_id: task_id,
            text: &text,
        },
    )
}

pub fn attempt_ends(conn: &Connection, task: &Task) -> Result<Vec<String>> {
    Ok(workers::attempts_at(conn, &task.task_id)?
        .into_iter()
        .filter(|w| w.attempt.is_some_and(|a| a > task.attempt_floor))
        .map(|w| w.end_reason.unwrap_or(w.status))
        .collect())
}

pub fn requeue_by_operator(conn: &Connection, task: &Task, reason: &str) -> Result<Task> {
    let held = [TaskStatus::Escalated, TaskStatus::Failed];
    if !is_in(&held, &task.status) && task.status != TaskStatus::Cancelled.as_str() {
        bail!(
            "{} is {}; only a held task is requeued",
            task.short(),
            task.shown_status()
        );
    }
    util::atomic(conn, |conn| requeue_by_operator_in(conn, task, reason))
}

fn requeue_by_operator_in(conn: &Connection, task: &Task, reason: &str) -> Result<Task> {
    let now = util::now();
    if task.is_parked() {
        resume_parked_worktree(conn, task)?;
    }
    let now_row = get(conn, &task.task_id)?;
    let was_cancelled = now_row.status == TaskStatus::Cancelled.as_str();
    let replan = now_row.status == TaskStatus::Failed.as_str();
    let text = if replan {
        format!(
            "{} is queued again for its 1 replan: {reason}",
            task.short()
        )
    } else {
        format!("{} is queued again by the operator: {reason}", task.short())
    };
    util::transition(
        conn,
        Entity::Task,
        &task.task_id,
        TaskStatus::Ready.as_str(),
        Actor::Orchestrator,
        util::Why::Event(EventKind::Requeued, &text),
    )?;
    conn.execute(
        "UPDATE tasks SET held_reason = NULL, attempt_floor = attempts,
            current_worker_id = NULL, summary = CASE WHEN ?3 THEN NULL ELSE summary END,
            finished_at = NULL, not_before = NULL, due_at = NULL,
            replans = replans + CASE WHEN ?4 THEN 1 ELSE 0 END, updated_at = ?2
         WHERE task_id = ?1",
        params![task.task_id, now, was_cancelled, replan],
    )?;
    needs_you::resolve(conn, &escalated_key(&task.task_id))?;
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
    get(conn, &task.task_id)
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersonaPlan {
    pub persona: String,
    pub startable: i64,
    pub on_the_way: i64,
    pub spawn: i64,
    pub replaces: Vec<String>,
}

pub fn plan_workers(conn: &Connection, run_id: &str) -> Result<Vec<PersonaPlan>> {
    plan_workers_except(conn, run_id, None)
}

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
        if let Some(last) = workers::attempts_at(conn, &t.task_id)?.last() {
            if last.status == WorkerStatus::Incomplete.as_str() {
                entry.1.push(last.worker_id.clone());
            }
        }
    }
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

pub fn spawn_refusal(conn: &Connection, run_id: &str, worker_id: &str) -> Result<Option<String>> {
    let w = workers::get(conn, worker_id)?;
    let Some(persona) = w.persona.as_deref() else {
        return Ok(None);
    };
    let plan = plan_workers_except(conn, run_id, Some(worker_id))?;
    let line = plan.iter().find(|p| p.persona == persona);
    let startable = line.map_or(0, |p| p.startable);
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

    fn store_with_run(dir: &std::path::Path) -> (Store, Run) {
        let mut store = Store::open(&dir.join("ledger.sqlite3")).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| {
                let run = runs::create(tx, dir, &new_run)?;
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        (store, run)
    }

    const TEXT: &str = "# 01: A\n\n<!-- horch-ticket: 1 -->\n**Persona:** primary `dev`\n";

    pub(super) fn spawn_for(
        store: &mut Store,
        run: &Run,
        persona: &str,
        target: Option<&str>,
    ) -> String {
        let new = NewWorker {
            run_id: run.run_id.clone(),
            persona: persona.into(),
            target_task_id: target.map(str::to_string),
            ..Default::default()
        };
        store.immediate(|tx| workers::insert(tx, &new)).unwrap()
    }

    fn bind(store: &mut Store, run: &Run, task: &str, persona: &str) -> String {
        let new = NewWorker {
            run_id: run.run_id.clone(),
            persona: persona.into(),
            ..Default::default()
        };
        crate::taskledger::testing::bind_live(store.conn(), task, &new, None)
            .unwrap()
            .worker_id
    }

    fn blocked_task(store: &mut Store, run: &Run) -> (String, String) {
        use crate::fleetd::ideas::application::questions::{ask, Ask};
        let mut new = NewTask::work("t", "t.md", "dev");
        new.worktree_mode = WorktreeMode::Shared;
        let task = store
            .immediate(|tx| add(tx, run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id;
        let first = bind(store, run, &task, "dev");
        let a = Ask {
            task_id: &task,
            attempt_id: "A1",
            mail_id: Some("m1"),
            text: "QUESTION: which file?\u{1b}[2J",
            asked_at: "2026-11-17T00:00:00Z",
        };
        ask(store.conn(), &a, FleetSeconds(0), 900).unwrap();
        store
            .conn()
            .execute(
                "UPDATE tasks SET status = 'blocked_on_user' WHERE task_id = ?1",
                [&task],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "UPDATE workers SET status = 'incomplete' WHERE worker_id = ?1",
                [&first],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "UPDATE attempts SET state = 'ended', outcome = 'asked' WHERE task_id = ?1",
                [&task],
            )
            .unwrap();
        (task, first)
    }

    #[test]
    fn answer_moves_a_blocked_on_user_task_to_ready() {
        use crate::fleetd::ideas::application::questions::answer_mail;
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let (task, _) = blocked_task(&mut store, &run);
        answer_mail(store.conn(), "m1", "src/a.rs").unwrap();
        let t = get(store.conn(), &task).unwrap();
        assert_eq!(t.status, TaskStatus::Ready.as_str());
        assert_eq!(t.current_worker_id, None);
        let cause: String = store
            .conn()
            .query_row(
                "SELECT cause FROM events WHERE kind = 'transition' AND entity = 'task'
                 AND entity_id = ?1 ORDER BY event_id DESC LIMIT 1",
                [&task],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cause, "question_answered: A1/Q1");
    }

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

        let reviewer = spawn_for(&mut store, &run, "opus-reviewer", Some(&review));
        assert_eq!(
            spawn(&plan_workers(store.conn(), &run.run_id).unwrap()),
            0,
            "the review has its worker on the way"
        );
        store
            .conn()
            .execute(
                "UPDATE workers SET status = 'claiming', last_checkin_at = '2000-01-01T00:00:00Z'
                 WHERE worker_id = ?1",
                [&reviewer],
            )
            .unwrap();
        assert_eq!(spawn(&plan_workers(store.conn(), &run.run_id).unwrap()), 0);

        store
            .conn()
            .execute(
                "UPDATE workers SET status = ?2 WHERE worker_id = ?1",
                params![reviewer, WorkerStatus::NoTask.as_str()],
            )
            .unwrap();
        assert_eq!(spawn(&plan_workers(store.conn(), &run.run_id).unwrap()), 1);
        let mine = spawn_for(&mut store, &run, "codex-reviewer", Some(&review));
        assert_eq!(
            spawn_refusal(store.conn(), &run.run_id, &mine).unwrap(),
            None
        );
    }

    #[test]
    fn trf_12_add_stores_the_snapshot_and_its_sha() {
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
        let task_json = serde_json::to_value(&t).unwrap();
        assert!(task_json.get("ticket_snapshot").is_none(), "{task_json}");
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

    fn hire(store: &mut Store, run: &Run, persona: &str) -> String {
        let new = NewWorker {
            run_id: run.run_id.clone(),
            persona: persona.into(),
            ..Default::default()
        };
        store.immediate(|tx| workers::insert(tx, &new)).unwrap()
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

    fn handing_off(store: &mut Store, run: &Run, dir: &std::path::Path) -> (String, Task, String) {
        queue(store, run, "a", "dev", |_| {});
        let b = queue(store, run, "b", "dev", |n| {
            n.priority = 50;
            n.worktree_mode = WorktreeMode::Shared;
        });
        let w = bind(store, run, &b.task_id, "dev");
        let task = get(store.conn(), &b.task_id).unwrap();
        store
            .immediate(|tx| {
                tx.execute(
                    "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path,
                        branch, base, status, state, origin, created_at)
                     VALUES ('wt1', ?1, ?2, '/repo', '/repo/.worktrees/b', 'fleet/b', 'main',
                        'open', 'active', 'native', ?3)",
                    params![task.task_id, w, util::now()],
                )?;
                Ok(())
            })
            .unwrap();
        let path = dir.join("opus-1-whats-next.md");
        std::fs::write(&path, "# Handoff\n").unwrap();
        (w, task, path.to_string_lossy().into_owned())
    }

    #[test]
    fn trf_37_handoff_ends_the_worker_and_requeues_the_task_first_with_its_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = store_with_run(dir.path());
        let (w, b, path) = handing_off(&mut store, &run, dir.path());
        let out = store.immediate(|tx| handoff(tx, &w, &path)).unwrap();
        assert_eq!(out.worker.status, WorkerStatus::Incomplete.as_str());
        assert_eq!(out.worker.end_reason.as_deref(), Some("handed_off"));
        assert_eq!(out.worker.handoff_path.as_deref(), Some(path.as_str()));
        assert!(out.worker.ended_at.is_some());
        assert_eq!(out.task.status, TaskStatus::Ready.as_str());
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
        let plan = plan_workers(store.conn(), &run.run_id).unwrap();
        assert_eq!(plan[0].replaces, vec![w.clone()]);
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
        let worker = workers::get(store.conn(), &w).unwrap();
        assert_eq!(worker.status, WorkerStatus::Working.as_str());
        let idle = hire(&mut store, &run, "dev");
        let err = store.immediate(|tx| handoff(tx, &idle, &path)).unwrap_err();
        assert!(format!("{err:#}").contains("holds no task"), "{err:#}");
        store.immediate(|tx| handoff(tx, &w, &path)).unwrap();
        assert!(store.immediate(|tx| handoff(tx, &w, &path)).is_err());
    }
}

#[cfg(test)]
mod attempt_tests {
    use super::*;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::workers::NewWorker;
    use crate::taskledger::Store;

    fn ledger(dir: &std::path::Path) -> (Store, Run) {
        let mut store = Store::open(&dir.join("ledger.sqlite3")).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| {
                let run = runs::create(tx, dir, &new_run)?;
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        (store, run)
    }

    fn add_task(store: &mut Store, run: &Run, title: &str) -> String {
        let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
        store
            .immediate(|tx| add(tx, run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id
    }

    fn bind_dev(store: &Store, run: &Run, task: &str) {
        let new = NewWorker {
            run_id: run.run_id.clone(),
            persona: "dev".into(),
            ..Default::default()
        };
        crate::taskledger::testing::bind_live(store.conn(), task, &new, None).unwrap();
    }

    fn add_after(store: &mut Store, run: &Run, title: &str, blocked_by: &[&str]) -> Task {
        let mut new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
        new.blocked_by = blocked_by.iter().map(|b| b.to_string()).collect();
        store
            .immediate(|tx| add(tx, run, &new, Actor::Orchestrator))
            .unwrap()
    }

    fn status_of(store: &Store, task_id: &str) -> String {
        get(store.conn(), task_id).unwrap().status
    }

    #[test]
    fn new_task_with_an_unmet_dependency_is_pending() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = ledger(dir.path());
        let a = add_task(&mut store, &run, "a");
        let b = add_after(&mut store, &run, "b", &["T-1"]);
        assert_eq!(status_of(&store, &a), "ready");
        assert_eq!(b.status, "pending");
        store
            .conn()
            .execute("UPDATE tasks SET status = 'done' WHERE task_id = ?1", [&a])
            .unwrap();
        let c = add_after(&mut store, &run, "c", &["T-1"]);
        assert_eq!(c.status, "ready");
        let mut first = NewTask::work("d", "tickets/d.md", "dev");
        first.blocked_by = vec!["T-2".into()];
        let mut second = NewTask::work("e", "tickets/e.md", "dev");
        second.blocked_by = vec!["d".into()];
        let third = NewTask::work("f", "tickets/f.md", "dev");
        let items = vec![
            ("d".to_string(), first),
            ("e".to_string(), second),
            ("f".to_string(), third),
        ];
        let added = store
            .immediate(|tx| add_many(tx, &run, &items, Actor::Orchestrator))
            .unwrap();
        let words: Vec<&str> = added.iter().map(|t| t.status.as_str()).collect();
        assert_eq!(words, ["pending", "pending", "ready"]);
    }

    #[test]
    fn done_moves_each_pending_dependent_with_all_dependencies_met_to_ready() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = ledger(dir.path());
        let a = shared_task(&mut store, &run, "a");
        let other = add_task(&mut store, &run, "other");
        let b = add_after(&mut store, &run, "b", &["T-1"]).task_id;
        let c = add_after(&mut store, &run, "c", &["T-1", "T-2"]).task_id;
        bind_dev(&store, &run, &a);
        store
            .immediate(|tx| {
                let why = util::Why::Event(EventKind::Done, "a is done");
                util::transition(tx, Entity::Task, &a, "done", Actor::Worker, why)?;
                ready_dependents(tx, &a)
            })
            .unwrap();
        assert_eq!(status_of(&store, &b), "ready");
        assert_eq!(status_of(&store, &c), "pending", "c waits for {other}");
        let moved: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM events WHERE kind = 'transition' AND entity = 'task'
                     AND entity_id = ?1 AND from_state = 'pending' AND to_state = 'ready'",
                [&b],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(moved, 1, "the move goes through the 1 door");
    }

    fn request_cancel(store: &Store, task_id: &str) {
        store
            .conn()
            .execute(
                "UPDATE tasks SET cancel_requested = 'dropped' WHERE task_id = ?1",
                [task_id],
            )
            .unwrap();
    }

    #[test]
    fn rung_6_skips_a_task_with_a_cancel_requested() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, run) = ledger(dir.path());
        let task = shared_task(&mut store, &run, "a");
        bind_dev(&store, &run, &task);
        request_cancel(&store, &task);

        let how = store
            .immediate(|tx| requeue(tx, &task, "its worker ended"))
            .unwrap();

        assert_eq!(how, Requeue::Skipped);
        assert_eq!(status_of(&store, &task), "running");
        assert_eq!(get(store.conn(), &task).unwrap().attempts, 1);
    }

    fn shared_task(store: &mut Store, run: &Run, title: &str) -> String {
        let mut new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
        new.worktree_mode = WorktreeMode::Shared;
        new.ticket_snapshot = Some(format!("# {title}\n"));
        store
            .immediate(|tx| add(tx, run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id
    }
}
