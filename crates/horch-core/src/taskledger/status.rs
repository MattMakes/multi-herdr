//! `horch ledger status` and `show`: the board of a run (section 5.6).

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::Serialize;

use crate::exeid::{exe_identity, ExeIdentity};

use super::runs::Run;
use super::store::optional;
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
    /// The watchdog's version and binary, once it has checked in (#216).
    pub watchdog: Option<WatchdogLine>,
    /// The run's parked worktrees on disk (#308).
    pub parked: Parked,
}

/// The parked worktrees of a run: on disk, and no step takes them to an
/// outcome. `horch worktree triage` decides them.
///
/// The disk is the sum of what `horch worktree triage` stored for each row.
/// `status` runs often, so it never walks a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Parked {
    pub count: usize,
    /// The sum of the stored measurements.
    pub bytes: u64,
    /// The rows with no stored measurement.
    pub unmeasured: usize,
    /// Seconds since the oldest stored measurement; `None` when no row has one.
    pub measured_age_s: Option<i64>,
}

impl Parked {
    fn of(conn: &Connection, run_id: &str, now: &str) -> Result<Parked> {
        let mut parked = Parked::default();
        let mut oldest: Option<String> = None;
        for path in super::runs::parked_worktrees(conn, run_id)? {
            parked.count += 1;
            let stored: Option<(i64, String)> = optional(conn.query_row(
                "SELECT bytes, bytes_measured_at FROM worktrees
                     WHERE path = ?1 AND bytes IS NOT NULL AND bytes_measured_at IS NOT NULL",
                [&path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ))?;
            match stored {
                Some((bytes, at)) => {
                    parked.bytes += bytes.max(0) as u64;
                    // Stamps sort as time does.
                    if oldest.as_ref().is_none_or(|o| at < *o) {
                        oldest = Some(at);
                    }
                }
                None => parked.unmeasured += 1,
            }
        }
        parked.measured_age_s = oldest.and_then(|at| util::seconds_between(&at, now));
        Ok(parked)
    }
}

/// The watchdog of a run, as it recorded itself at its last checkin.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WatchdogLine {
    pub version: String,
    pub exe: Option<ExeIdentity>,
    pub checkin_at: Option<String>,
    /// It runs another binary than the horch that built this board: an old
    /// watchdog after `horch self-update`.
    pub other_binary: bool,
}

impl WatchdogLine {
    /// The line for `run`, judged against the binary `current` that builds
    /// the board. A run whose watchdog never checked in has none.
    fn of(run: &Run, current: Option<&ExeIdentity>) -> Option<WatchdogLine> {
        let version = run.watchdog_version.clone()?;
        Some(WatchdogLine {
            other_binary: run
                .watchdog_exe
                .as_ref()
                .zip(current)
                .is_some_and(|(a, b)| a != b),
            exe: run.watchdog_exe.clone(),
            checkin_at: run.watchdog_checkin_at.clone(),
            version,
        })
    }
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
        .query_map([run_id], WorktreeLine::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The columns of the `worktrees` SELECT.
type WorktreeRow = (
    String,
    Option<i64>,
    String,
    String,
    String,
    Option<i64>,
    Option<String>,
    String,
    i64,
);

impl WorktreeLine {
    /// The 9 columns of the `worktrees` SELECT, in order.
    fn from_row(r: &Row<'_>) -> rusqlite::Result<WorktreeLine> {
        let (worktree_id, seq, path, branch, status, bytes_on_disk, reason, base, scratch): WorktreeRow =
            r.try_into()?;
        Ok(WorktreeLine {
            worktree_id,
            task: seq.map(|s| format!("T-{s}")),
            path,
            branch,
            base,
            scratch: scratch != 0,
            status,
            bytes_on_disk,
            reason,
        })
    }
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

impl Counts {
    /// Count 1 task by its status; a status with no counter is not counted.
    fn add(&mut self, status: &str) {
        match status {
            "queued" => self.queued += 1,
            "running" => self.running += 1,
            "landing" => self.landing += 1,
            "done" => self.done += 1,
            "cancelled" => self.cancelled += 1,
            _ => {}
        }
    }
}

/// Why a task does not start now: its hold, else the queue's wait.
fn task_wait(t: &Task, waits: &[(Task, tasks::Wait)]) -> Option<String> {
    match &t.held_reason {
        Some(r) => Some(format!(
            "held: {r}. Ask the operator; release it with horch ledger requeue {} \"<reason>\"",
            t.short()
        )),
        None => waits
            .iter()
            .find(|(w, _)| w.task_id == t.task_id)
            .map(|(_, w)| w.to_string()),
    }
}

/// The line of 1 worker: its ages, its watch state and its waits.
fn worker_line(
    conn: &Connection,
    run: &Run,
    w: Worker,
    lines: &[TaskLine],
    now: &str,
) -> Result<WorkerLine> {
    let checkin_age_s = w
        .last_checkin_at
        .as_deref()
        .and_then(|t| util::seconds_between(t, now));
    let progress_age_s = w
        .last_progress_at
        .as_deref()
        .and_then(|t| util::seconds_between(t, now));
    let task = w.task_id.as_ref().and_then(|id| {
        lines
            .iter()
            .find(|l| &l.task.task_id == id)
            .map(|l| l.task.short())
    });
    Ok(WorkerLine {
        stale: workers::is_stale(&w, run.heartbeat_stale_s, now),
        waiting: workers::waiting(conn, &w.worker_id)?,
        unwatched: is_unwatched(conn, run, &w, now)?,
        worker: w,
        task,
        checkin_age_s,
        progress_age_s,
    })
}

/// A working worker with no check in the last 2 check intervals.
fn is_unwatched(conn: &Connection, run: &Run, w: &Worker, now: &str) -> Result<bool> {
    if w.status != "working" {
        return Ok(false);
    }
    let since = last_check_at(conn, &w.worker_id)?.or_else(|| w.claimed_at.clone());
    Ok(since
        .as_deref()
        .and_then(|t| util::seconds_between(t, now))
        .is_some_and(|age| age > 2 * run.check_every_s))
}

/// The line of 1 task: its holder's role, its wait and how its attempts ended.
fn task_line(
    conn: &Connection,
    t: Task,
    waits: &[(Task, tasks::Wait)],
    roster: &[Worker],
) -> Result<TaskLine> {
    let attempt_ends = if t.attempts > 1 {
        tasks::attempt_ends(conn, &t)?
    } else {
        Vec::new()
    };
    let worker = t.current_worker_id.as_ref().and_then(|id| {
        roster
            .iter()
            .find(|w| &w.worker_id == id)
            .map(|w| w.label().to_string())
    });
    Ok(TaskLine {
        attempt_ends,
        worker,
        waits: task_wait(&t, waits),
        task: t,
    })
}

/// The lines of every worker of the run.
fn worker_lines(
    conn: &Connection,
    run: &Run,
    roster: Vec<Worker>,
    lines: &[TaskLine],
    now: &str,
) -> Result<Vec<WorkerLine>> {
    roster
        .into_iter()
        .map(|w| worker_line(conn, run, w, lines, now))
        .collect()
}

/// A worktree still holds work unless it is scratch or finished.
fn any_open_worktree(wts: &[WorktreeLine]) -> bool {
    wts.iter().any(|w| {
        !w.scratch
            && !matches!(
                w.status.as_str(),
                "cleaned" | "unlanded" | "abandoned" | "archived" | "trashed"
            )
    })
}

/// Build the board of `run`.
pub fn report(conn: &Connection, run: &Run) -> Result<Report> {
    let current = std::env::current_exe().ok().and_then(|p| exe_identity(&p));
    report_for(conn, run, current.as_ref())
}

/// Build the board of `run`, judging its watchdog against the binary
/// `current` that builds it.
pub fn report_for(conn: &Connection, run: &Run, current: Option<&ExeIdentity>) -> Result<Report> {
    let now = util::now();
    let waits = tasks::waits(conn, &run.run_id)?;
    let roster = workers::of_run(conn, &run.run_id)?;
    let mut counts = Counts::default();
    let mut lines = Vec::new();
    for t in tasks::of_run(conn, &run.run_id)? {
        counts.add(&t.status);
        lines.push(task_line(conn, t, &waits, &roster)?);
    }
    let wl = worker_lines(conn, run, roster, &lines, &now)?;
    let wts = worktrees(conn, &run.run_id)?;
    let open_tasks = counts.queued + counts.running + counts.landing;
    Ok(Report {
        run: run.clone(),
        tasks: lines,
        workers: wl,
        complete: open_tasks == 0 && !any_open_worktree(&wts),
        worktrees: wts,
        counts,
        watchdog: WatchdogLine::of(run, current),
        parked: Parked::of(conn, &run.run_id, &now)?,
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
    render_watchdog(&mut out, r.watchdog.as_ref());
    render_tasks(&mut out, &r.tasks);
    render_live(&mut out, &r.workers);
    render_open_worktrees(&mut out, &r.worktrees);
    render_parked(&mut out, &r.parked);
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

/// The watchdog line; nothing before its first checkin.
fn render_watchdog(out: &mut String, w: Option<&WatchdogLine>) {
    let Some(w) = w else { return };
    let _ = writeln!(
        out,
        "Watchdog: horch {}, last checkin {}.{}",
        w.version,
        w.checkin_at.as_deref().unwrap_or("-"),
        if w.other_binary {
            " OLD BINARY: it runs another binary than this horch. Restart it with horch install."
        } else {
            ""
        }
    );
}

/// The task rows, or `No tasks.`.
fn render_tasks(out: &mut String, lines: &[TaskLine]) {
    if lines.is_empty() {
        let _ = writeln!(out, "No tasks.");
    }
    for l in lines {
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
}

/// The live workers block, with their waits; nothing when none is live.
fn render_live(out: &mut String, workers: &[WorkerLine]) {
    let live: Vec<&WorkerLine> = workers
        .iter()
        .filter(|w| {
            matches!(
                w.worker.status.as_str(),
                "spawning" | "starting" | "claiming" | "working"
            ) && w.worker.kind == "worker"
        })
        .collect();
    if live.is_empty() {
        return;
    }
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

/// The parked worktrees line; nothing when none.
fn render_parked(out: &mut String, p: &Parked) {
    if p.count == 0 {
        return;
    }
    let disk = match p.measured_age_s {
        Some(age) => {
            let unmeasured = match p.unmeasured {
                0 => String::new(),
                n => format!("; {n} not measured"),
            };
            format!(
                "{}, measured {} ago{unmeasured}",
                super::triage::human_bytes(p.bytes),
                ago(age)
            )
        }
        None => "disk not measured; run horch worktree triage".to_string(),
    };
    let _ = writeln!(
        out,
        "Parked worktrees: {} ({disk}). Decide about them with horch worktree triage.",
        p.count,
    );
}

/// How long ago, in the largest whole unit: `45s`, `5m`, `2h`, `3d`.
fn ago(s: i64) -> String {
    match s.max(0) {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// The worktrees block for every worktree not yet cleaned; nothing when none.
fn render_open_worktrees(out: &mut String, worktrees: &[WorktreeLine]) {
    let open: Vec<&WorktreeLine> = worktrees
        .iter()
        .filter(|w| !super::triage::SETTLED.contains(&w.status.as_str()))
        .collect();
    if open.is_empty() {
        return;
    }
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
            r.try_into().map(|(at, actor, kind, text)| EventLine {
                at,
                actor,
                kind,
                text,
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

/// The columns of the `stops` SELECT.
type StopRow = (
    String,
    String,
    String,
    String,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    String,
);

impl StopLine {
    /// The 9 columns of the `stops` SELECT, in order.
    fn from_row(r: &Row<'_>) -> rusqlite::Result<StopLine> {
        let (at, requested_by, reason, target, pid, pid_started, signal, pane_id, result): StopRow =
            r.try_into()?;
        Ok(StopLine {
            at,
            requested_by,
            reason,
            target,
            pid,
            pid_started,
            signal,
            pane_id,
            result,
        })
    }
}

/// The stops of 1 worker, oldest first.
pub fn stops(conn: &Connection, worker_id: &str) -> Result<Vec<StopLine>> {
    let mut stmt = conn.prepare(
        "SELECT at, requested_by, reason, target, pid, pid_started, signal, pane_id, result
         FROM stops WHERE worker_id = ?1 ORDER BY stop_id",
    )?;
    let rows = stmt
        .query_map([worker_id], StopLine::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
