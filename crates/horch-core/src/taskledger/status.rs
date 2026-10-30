//! `horch ledger status` and `show`: the board of a run (section 5.6).

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use super::runs::Run;
use super::tasks::{self, Task};
use super::util;
use super::workers::{self, Worker};

/// 1 worktree line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorktreeLine {
    pub worktree_id: String,
    pub task: Option<String>,
    pub path: String,
    pub branch: String,
    pub base: String,
    pub scratch: bool,
    pub status: String,
    pub bytes_on_disk: Option<i64>,
    pub reason: Option<String>,
}

/// 1 task line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskLine {
    pub task: Task,
    /// The role of the worker that holds it.
    pub worker: Option<String>,
    /// Why a queued task does not start now.
    pub waits: Option<String>,
    /// How each attempt since the last operator requeue ended, when the
    /// task had more than 1 attempt (#168).
    pub attempt_ends: Vec<String>,
}

/// 1 worker line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkerLine {
    pub worker: Worker,
    pub task: Option<String>,
    pub checkin_age_s: Option<i64>,
    pub progress_age_s: Option<i64>,
    /// A working worker with no check in the last 2 check intervals.
    pub unwatched: bool,
    /// A pre-claim row whose process went silent ([`workers::is_stale`]).
    pub stale: bool,
    pub waiting: Vec<workers::Wait>,
}

/// The board of 1 run.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub run: Run,
    pub tasks: Vec<TaskLine>,
    pub workers: Vec<WorkerLine>,
    pub worktrees: Vec<WorktreeLine>,
    /// Every task done or cancelled, and every worktree cleaned, unlanded or
    /// abandoned: nothing is left to do.
    pub complete: bool,
    pub counts: Counts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub queued: i64,
    pub running: i64,
    pub landing: i64,
    pub done: i64,
    pub cancelled: i64,
}

/// The worktrees of a run.
pub fn worktrees(conn: &Connection, run_id: &str) -> Result<Vec<WorktreeLine>> {
    let mut stmt = conn.prepare(
        "SELECT w.worktree_id, t.seq, w.path, w.branch, w.status, w.bytes_on_disk, w.reason, w.base, COALESCE(m.scratch, 0)
         FROM worktrees w LEFT JOIN tasks t ON t.task_id = w.task_id
         LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
         WHERE t.run_id = ?1 OR m.run_id = ?1 ORDER BY w.created_at, w.rowid",
    )?;
    let rows = stmt
        .query_map([run_id], |r| {
            Ok(WorktreeLine {
                worktree_id: r.get(0)?,
                task: r.get::<_, Option<i64>>(1)?.map(|s| format!("T-{s}")),
                path: r.get(2)?,
                branch: r.get(3)?,
                status: r.get(4)?,
                bytes_on_disk: r.get(5)?,
                reason: r.get(6)?,
                base: r.get(7)?,
                scratch: r.get::<_, i64>(8)? != 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Every path the ledger records as a worktree, in any run. The claim
/// registry never sees these, so the unregistered scan asks for them here.
pub fn worktree_paths(conn: &Connection) -> Result<Vec<PathBuf>> {
    let mut stmt = conn.prepare("SELECT path FROM worktrees")?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().map(PathBuf::from).collect())
}

/// The last check of a worker, if any.
fn last_check_at(conn: &Connection, worker_id: &str) -> Result<Option<String>> {
    super::store::optional(conn.query_row(
        "SELECT at FROM checks WHERE worker_id = ?1 ORDER BY check_id DESC LIMIT 1",
        params![worker_id],
        |r| r.get(0),
    ))
}

/// Build the board of `run`.
pub fn report(conn: &Connection, run: &Run) -> Result<Report> {
    let now = util::now();
    let all = tasks::of_run(conn, &run.run_id)?;
    let waits = tasks::waits(conn, &run.run_id)?;
    let roster = workers::of_run(conn, &run.run_id)?;
    let role_of = |id: &Option<String>| {
        id.as_ref().and_then(|id| {
            roster
                .iter()
                .find(|w| &w.worker_id == id)
                .map(|w| w.label().to_string())
        })
    };
    let mut counts = Counts::default();
    let mut lines = Vec::new();
    for t in all {
        match t.status.as_str() {
            "queued" => counts.queued += 1,
            "running" => counts.running += 1,
            "landing" => counts.landing += 1,
            "done" => counts.done += 1,
            "cancelled" => counts.cancelled += 1,
            _ => {}
        }
        let wait = match &t.held_reason {
            Some(r) => Some(format!(
                "held: {r}. Ask the operator; release it with horch ledger requeue {} \"<reason>\"",
                t.short()
            )),
            None => waits
                .iter()
                .find(|(w, _)| w.task_id == t.task_id)
                .map(|(_, w)| w.to_string()),
        };
        let attempt_ends = if t.attempts > 1 {
            tasks::attempt_ends(conn, &t)?
        } else {
            Vec::new()
        };
        lines.push(TaskLine {
            attempt_ends,
            worker: role_of(&t.current_worker_id),
            waits: wait,
            task: t,
        });
    }
    let mut wl = Vec::new();
    for w in roster {
        let checkin_age_s = w
            .last_checkin_at
            .as_deref()
            .and_then(|t| util::seconds_between(t, &now));
        let progress_age_s = w
            .last_progress_at
            .as_deref()
            .and_then(|t| util::seconds_between(t, &now));
        let unwatched = if w.status == "working" {
            let since = last_check_at(conn, &w.worker_id)?.or_else(|| w.claimed_at.clone());
            since
                .as_deref()
                .and_then(|t| util::seconds_between(t, &now))
                .is_some_and(|age| age > 2 * run.check_every_s)
        } else {
            false
        };
        let task = w.task_id.as_ref().and_then(|id| {
            lines
                .iter()
                .find(|l| &l.task.task_id == id)
                .map(|l| l.task.short())
        });
        wl.push(WorkerLine {
            stale: workers::is_stale(&w, run.heartbeat_stale_s, &now),
            waiting: workers::waiting(conn, &w.worker_id)?,
            worker: w,
            task,
            checkin_age_s,
            progress_age_s,
            unwatched,
        });
    }
    let wts = worktrees(conn, &run.run_id)?;
    let open_tasks = counts.queued + counts.running + counts.landing;
    let open_worktrees = wts
        .iter()
        .any(|w| !w.scratch && !matches!(w.status.as_str(), "cleaned" | "unlanded" | "abandoned"));
    Ok(Report {
        run: run.clone(),
        tasks: lines,
        workers: wl,
        worktrees: wts,
        complete: open_tasks == 0 && !open_worktrees,
        counts,
    })
}

fn age(s: Option<i64>) -> String {
    match s {
        None => "-".into(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m", s / 60),
        Some(s) => format!("{}h{}m", s / 3600, (s % 3600) / 60),
    }
}

/// The text form of a board.
pub fn render(r: &Report) -> String {
    let mut out = String::new();
    let run = &r.run;
    let _ = writeln!(
        out,
        "Run {} ({}). Branch: {} -> {}. PR: {}.",
        run.slug,
        run.status,
        run.run_branch.as_deref().unwrap_or("(none)"),
        run.base_branch.as_deref().unwrap_or("(none)"),
        run.pr_url.as_deref().unwrap_or("none")
    );
    if r.tasks.is_empty() {
        let _ = writeln!(out, "No tasks.");
    }
    for l in &r.tasks {
        let t = &l.task;
        let _ = write!(
            out,
            "{:<5} {:<9} {:<18} {:<20} {}",
            t.short(),
            t.status,
            t.claimable_by.as_deref().unwrap_or("-"),
            l.worker.as_deref().unwrap_or("-"),
            t.title
        );
        if let Some(w) = &l.waits {
            let _ = write!(out, "  [{w}]");
        }
        if t.attempts > 1 {
            let _ = write!(out, "  [{} attempts", t.attempts);
            if !l.attempt_ends.is_empty() {
                let _ = write!(out, "; ends: {}", l.attempt_ends.join("; "));
            }
            let _ = write!(out, "]");
        }
        let _ = writeln!(out);
    }
    let live: Vec<&WorkerLine> = r
        .workers
        .iter()
        .filter(|w| {
            matches!(
                w.worker.status.as_str(),
                "spawning" | "starting" | "claiming" | "working"
            ) && w.worker.kind == "worker"
        })
        .collect();
    if !live.is_empty() {
        let _ = writeln!(out, "Live workers:");
        for w in live {
            for wait in &w.waiting {
                let _ = writeln!(
                    out,
                    "  {} waiting: {} ({} ago; until {})",
                    w.worker.label(),
                    wait.reason,
                    age(util::seconds_between(&wait.started_at, &util::now())),
                    wait.deadline
                );
            }
            let _ = writeln!(
                out,
                "  {:<20} {:<9} {:<5} checkin {} ago, progress {} ago{}",
                w.worker.label(),
                w.worker.status,
                w.task.as_deref().unwrap_or("-"),
                age(w.checkin_age_s),
                age(w.progress_age_s),
                if w.unwatched {
                    "  UNWATCHED"
                } else if w.stale {
                    "  STALE"
                } else {
                    ""
                }
            );
        }
    }
    let open: Vec<&WorktreeLine> = r
        .worktrees
        .iter()
        .filter(|w| w.status != "cleaned")
        .collect();
    if !open.is_empty() {
        let _ = writeln!(out, "Worktrees:");
        for w in open {
            let _ = writeln!(
                out,
                "  {:<5} {:<14} {} (base {}){}{}",
                w.task.as_deref().unwrap_or("-"),
                w.status,
                w.path,
                w.base,
                if w.scratch { " [scratch]" } else { "" },
                w.reason
                    .as_deref()
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default()
            );
        }
    }
    let c = &r.counts;
    let _ = writeln!(
        out,
        "{} tasks: {} done, {} running, {} landing, {} queued, {} cancelled. {}",
        r.tasks.len(),
        c.done,
        c.running,
        c.landing,
        c.queued,
        c.cancelled,
        if r.complete {
            "Complete."
        } else {
            "Not complete."
        }
    );
    out
}

/// The history of 1 task or worker, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventLine {
    pub at: String,
    pub actor: String,
    pub kind: String,
    pub text: String,
}

/// The events about a task (`column = "task_id"`) or a worker.
pub fn events(conn: &Connection, column: &str, id: &str) -> Result<Vec<EventLine>> {
    let column = match column {
        "task_id" => "task_id",
        "run_id" => "run_id",
        _ => "worker_id",
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT at, actor, kind, text FROM events WHERE {column} = ?1 ORDER BY event_id"
    ))?;
    let rows = stmt
        .query_map([id], |r| {
            Ok(EventLine {
                at: r.get(0)?,
                actor: r.get(1)?,
                kind: r.get(2)?,
                text: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 1 recorded stop step.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StopLine {
    pub at: String,
    pub requested_by: String,
    pub reason: String,
    pub target: String,
    pub pid: Option<i64>,
    pub pid_started: Option<i64>,
    pub signal: Option<String>,
    pub pane_id: Option<String>,
    pub result: String,
}

/// The stops of 1 worker, oldest first.
pub fn stops(conn: &Connection, worker_id: &str) -> Result<Vec<StopLine>> {
    let mut stmt = conn.prepare(
        "SELECT at, requested_by, reason, target, pid, pid_started, signal, pane_id, result
         FROM stops WHERE worker_id = ?1 ORDER BY stop_id",
    )?;
    let rows = stmt
        .query_map([worker_id], |r| {
            Ok(StopLine {
                at: r.get(0)?,
                requested_by: r.get(1)?,
                reason: r.get(2)?,
                target: r.get(3)?,
                pid: r.get(4)?,
                pid_started: r.get(5)?,
                signal: r.get(6)?,
                pane_id: r.get(7)?,
                result: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
