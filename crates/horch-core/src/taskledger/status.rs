use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::Serialize;

use crate::exeid::{exe_identity, ExeIdentity};
use crate::fleetd::fleet_runtime::adapters::clock_store;
use crate::fleetd::fleet_runtime::adapters::sqlite::generation;
use crate::fleetd::fleet_runtime::application::holds;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::integration::application::queries::{self as integration, Integration};
use crate::fleetd::worker_profiles::adapters::check_sandbox::NO_CHECK_SANDBOX_KIND;
use crate::fleetd::worker_profiles::adapters::sandbox_rules::NO_SANDBOX;
use crate::fleetd::worktree_custody::application::queries::{self as custody, DiskTotal};

use super::model::{TaskStatus, WorktreeStatus};
use super::runs::Run;
use super::sql_words;
use super::store::optional;
use super::tasks::{self, Task};
use super::util;
use super::workers::{self, Worker};

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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskLine {
    pub task: Task,
    pub worker: Option<String>,
    pub waits: Option<String>,
    pub attempt_ends: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkerLine {
    pub worker: Worker,
    pub task: Option<String>,
    pub checkin_age_s: Option<i64>,
    pub progress_age_s: Option<i64>,
    pub unwatched: bool,
    pub stale: bool,
    pub launch_pending: bool,
    pub waiting: Vec<workers::Wait>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub run: Run,
    pub tasks: Vec<TaskLine>,
    pub workers: Vec<WorkerLine>,
    pub worktrees: Vec<WorktreeLine>,
    pub complete: bool,
    pub unsettled: Vec<Unsettled>,
    pub counts: Counts,
    pub watchdog: Option<WatchdogLine>,
    pub parked: Parked,
    pub orchestrator: Option<String>,
    pub needs_you: NeedsYou,
    pub worktree_disk: DiskTotal,
    pub no_check_sandbox: Vec<String>,
    pub integration: Integration,
    pub dispatch_holds: Vec<String>,
    pub leases: Vec<LeaseLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LeaseLine {
    pub task: String,
    pub attempt: String,
    pub worker: String,
    pub renewed_at: String,
    pub renew_basis: String,
    pub expires_in_s: Option<i64>,
    pub last_signal: Option<String>,
    pub last_signal_at: Option<String>,
}

fn leases(conn: &Connection, run_id: &str) -> Result<Vec<LeaseLine>> {
    let clock = match generation(conn)? {
        Some(_) => Some(clock_store::read(conn)?.0),
        None => None,
    };
    let mut stmt = conn.prepare(
        "SELECT t.seq, l.attempt_id, COALESCE(w.role, w.worker_id), l.renewed_at,
                l.renew_basis, l.expires_at, w.last_progress_source, w.last_progress_at
         FROM leases l JOIN tasks t ON t.task_id = l.task_id
         JOIN attempts a ON a.id = l.attempt_id
         JOIN workers w ON w.worker_id = a.worker_id
         WHERE t.run_id = ?1 ORDER BY t.seq",
    )?;
    let rows = stmt.query_map(params![run_id], |r| {
        let expires_at: i64 = r.get(5)?;
        Ok(LeaseLine {
            task: format!("T-{}", r.get::<_, i64>(0)?),
            attempt: r.get(1)?,
            worker: r.get(2)?,
            renewed_at: r.get(3)?,
            renew_basis: r.get(4)?,
            expires_in_s: clock.map(|c| expires_at - c),
            last_signal: r.get(6)?,
            last_signal_at: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NeedsYou {
    pub open: usize,
    pub items: Vec<NeedsYouLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NeedsYouLine {
    pub key: String,
    pub kind: String,
    pub entity: String,
    pub entity_id: String,
    pub text: String,
    pub created_at: String,
}

impl NeedsYou {
    fn of(conn: &Connection) -> Result<NeedsYou> {
        let items: Vec<NeedsYouLine> = needs_you::open_items(conn)?
            .into_iter()
            .map(|i| NeedsYouLine {
                key: i.key,
                kind: i.kind,
                entity: i.entity,
                entity_id: i.entity_id,
                text: i.text,
                created_at: i.created_at,
            })
            .collect();
        Ok(NeedsYou {
            open: items.len(),
            items,
        })
    }

    fn no_check_sandbox(&self) -> Vec<String> {
        let prefix = format!("{NO_CHECK_SANDBOX_KIND}:");
        self.items
            .iter()
            .filter(|i| i.kind == NO_CHECK_SANDBOX_KIND)
            .map(|i| i.key.strip_prefix(&prefix).unwrap_or(&i.key).to_string())
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Parked {
    pub count: usize,
    pub bytes: u64,
    pub unmeasured: usize,
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
                     WHERE path = ?1 AND state <> 'removed'
                         AND bytes IS NOT NULL AND bytes_measured_at IS NOT NULL",
                [&path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ))?;
            match stored {
                Some((bytes, at)) => {
                    parked.bytes += bytes.max(0) as u64;
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WatchdogLine {
    pub version: String,
    pub exe: Option<ExeIdentity>,
    pub checkin_at: Option<String>,
    pub other_binary: bool,
}

impl WatchdogLine {
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

pub fn worktree_paths(conn: &Connection) -> Result<Vec<PathBuf>> {
    let mut stmt = conn.prepare("SELECT path FROM worktrees")?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().map(PathBuf::from).collect())
}

fn last_check_at(conn: &Connection, worker_id: &str) -> Result<Option<String>> {
    super::store::optional(conn.query_row(
        "SELECT at FROM checks WHERE worker_id = ?1 ORDER BY check_id DESC LIMIT 1",
        params![worker_id],
        |r| r.get(0),
    ))
}

impl Counts {
    fn add(&mut self, status: &str) {
        match TaskStatus::shown_word(status) {
            "queued" => self.queued += 1,
            "running" => self.running += 1,
            "landing" => self.landing += 1,
            "done" => self.done += 1,
            "cancelled" => self.cancelled += 1,
            _ => {}
        }
    }
}

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
    let launch_pending = w.kind == "orchestrator" && w.status == "spawning";
    Ok(WorkerLine {
        stale: !launch_pending && workers::is_stale(&w, run.heartbeat_stale_s, now),
        launch_pending,
        waiting: workers::waits_now(conn, &w.worker_id)?,
        unwatched: is_unwatched(conn, run, &w, now)?,
        worker: w,
        task,
        checkin_age_s,
        progress_age_s,
    })
}

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

fn task_line(
    conn: &Connection,
    t: Task,
    waits: &[(Task, tasks::Wait)],
    held: &BTreeMap<String, String>,
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
        waits: match held.get(&t.task_id) {
            Some(cause) if t.held_reason.is_none() => Some(format!("hold on dispatch: {cause}")),
            _ => task_wait(&t, waits),
        },
        task: t,
    })
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Unsettled {
    Task {
        task: String,
        status: String,
        held: Option<String>,
    },
    Worktree {
        path: String,
        status: String,
    },
    Parked {
        path: String,
        status: String,
    },
}

impl std::fmt::Display for Unsettled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unsettled::Task {
                task,
                held: Some(r),
                ..
            } => write!(f, "{task} (held: {r})"),
            Unsettled::Task { task, status, .. } => write!(f, "{task} ({status})"),
            Unsettled::Worktree { path, status } => write!(f, "worktree {path} ({status})"),
            Unsettled::Parked { path, status } => {
                write!(f, "worktree {path} ({status}, no triage decision)")
            }
        }
    }
}

fn worktree_unsettled(
    path: String,
    status: String,
    decision: Option<&str>,
    scratch: bool,
) -> Option<Unsettled> {
    if scratch {
        return None;
    }
    match WorktreeStatus::parse(&status) {
        Ok(WorktreeStatus::Cleaned | WorktreeStatus::Archived | WorktreeStatus::Trashed) => None,
        Ok(
            WorktreeStatus::Landed
            | WorktreeStatus::CleanupFailed
            | WorktreeStatus::Unlanded
            | WorktreeStatus::Abandoned,
        ) => (decision.is_none() && std::path::Path::new(&path).exists())
            .then_some(Unsettled::Parked { path, status }),
        Ok(WorktreeStatus::Open | WorktreeStatus::Landing) | Err(_) => {
            Some(Unsettled::Worktree { path, status })
        }
    }
}

pub fn unsettled(conn: &Connection, run_id: &str) -> Result<Vec<Unsettled>> {
    let mut out: Vec<Unsettled> = tasks::of_run(conn, run_id)?
        .into_iter()
        .filter(|t| {
            !TaskStatus::parse(&t.status).is_ok_and(|s| sql_words::TASKS_ENDED.contains(&s))
        })
        .map(|t| Unsettled::Task {
            task: t.short(),
            status: t.shown_status().to_string(),
            held: t.held_reason.clone(),
        })
        .collect();
    let mut stmt = conn.prepare(
        "SELECT w.path, w.status, w.decision, COALESCE(m.scratch, 0)
         FROM worktrees w LEFT JOIN tasks t ON t.task_id = w.task_id
         LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
         WHERE t.run_id = ?1 OR m.run_id = ?1 ORDER BY w.created_at, w.rowid",
    )?;
    let rows = stmt
        .query_map([run_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    out.extend(
        rows.into_iter()
            .filter_map(|(path, status, decision, scratch)| {
                worktree_unsettled(path, status, decision.as_deref(), scratch != 0)
            }),
    );
    Ok(out)
}

pub fn report(conn: &Connection, run: &Run) -> Result<Report> {
    report_with(conn, run, &|_| None)
}

pub fn report_with(
    conn: &Connection,
    run: &Run,
    pool_of: &dyn Fn(&str) -> Option<String>,
) -> Result<Report> {
    let current = std::env::current_exe().ok().and_then(|p| exe_identity(&p));
    board(conn, run, current.as_ref(), pool_of)
}

pub fn report_for(conn: &Connection, run: &Run, current: Option<&ExeIdentity>) -> Result<Report> {
    board(conn, run, current, &|_| None)
}

fn board(
    conn: &Connection,
    run: &Run,
    current: Option<&ExeIdentity>,
    pool_of: &dyn Fn(&str) -> Option<String>,
) -> Result<Report> {
    let now = util::now();
    let waits = tasks::waits(conn, &run.run_id)?;
    let held = holds::task_holds(conn, pool_of)?;
    let roster = workers::of_run(conn, &run.run_id)?;
    let mut counts = Counts::default();
    let mut lines = Vec::new();
    for t in tasks::of_run(conn, &run.run_id)? {
        counts.add(&t.status);
        lines.push(task_line(conn, t, &waits, &held, &roster)?);
    }
    let wl = worker_lines(conn, run, roster, &lines, &now)?;
    let wts = worktrees(conn, &run.run_id)?;
    let unsettled = unsettled(conn, &run.run_id)?;
    let needs_you = NeedsYou::of(conn)?;
    Ok(Report {
        run: run.clone(),
        tasks: lines,
        workers: wl,
        complete: unsettled.is_empty(),
        unsettled,
        worktrees: wts,
        counts,
        watchdog: WatchdogLine::of(run, current),
        parked: Parked::of(conn, &run.run_id, &now)?,
        orchestrator: super::orchestrator::status_line(conn, &run.run_id)?,
        no_check_sandbox: needs_you.no_check_sandbox(),
        needs_you,
        worktree_disk: custody::total_bytes(conn)?,
        integration: integration::integration(conn)?,
        dispatch_holds: holds::open_holds(conn)?
            .into_iter()
            .map(|h| h.cause)
            .collect(),
        leases: leases(conn, &run.run_id)?,
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
    if let Some(line) = &r.orchestrator {
        let _ = writeln!(out, "{line}");
    }
    for w in r.workers.iter().filter(|w| w.launch_pending) {
        let _ = writeln!(out, "Orchestrator {}: launch pending.", w.worker.label());
    }
    render_tasks(&mut out, &r.tasks);
    render_live(&mut out, &r.workers);
    render_leases(&mut out, &r.leases);
    for cause in &r.dispatch_holds {
        let _ = writeln!(out, "Hold on dispatch: {cause}.");
    }
    render_open_worktrees(&mut out, &r.worktrees);
    render_parked(&mut out, &r.parked);
    render_worktree_disk(&mut out, &r.worktree_disk);
    render_integration(&mut out, &r.integration);
    render_needs_you(&mut out, &r.no_check_sandbox, &r.needs_you);
    if !r.unsettled.is_empty() {
        let items: Vec<String> = r.unsettled.iter().map(|u| u.to_string()).collect();
        let _ = writeln!(out, "Unsettled: {}.", items.join(", "));
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

fn render_needs_you(out: &mut String, no_check_sandbox: &[String], n: &NeedsYou) {
    if n.items.is_empty() {
        return;
    }
    for persona in no_check_sandbox {
        let _ = writeln!(out, "Profile {persona}: {NO_SANDBOX}.");
    }
    let _ = writeln!(out, "Needs you ({}):", n.open);
    for i in &n.items {
        let _ = writeln!(out, "  {}: {}", i.key, i.text);
    }
}

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
            t.shown_status(),
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

fn render_leases(out: &mut String, leases: &[LeaseLine]) {
    for l in leases {
        let expires = match l.expires_in_s {
            Some(s) => format!("expires in {s} s"),
            None => "no fleet clock".to_string(),
        };
        let signal = match (&l.last_signal, &l.last_signal_at) {
            (Some(s), Some(at)) => format!("last signal {s} at {at}"),
            _ => "no signal yet".to_string(),
        };
        let _ = writeln!(
            out,
            "Lease of {}: attempt {} of {}, renewed at {} by {}, {expires}; {signal}.",
            l.task, l.attempt, l.worker, l.renewed_at, l.renew_basis
        );
    }
}

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

fn render_worktree_disk(out: &mut String, d: &DiskTotal) {
    if d.count == 0 {
        return;
    }
    let unmeasured = match d.unmeasured {
        0 => String::new(),
        n => format!("; {n} not measured"),
    };
    let _ = writeln!(
        out,
        "Worktree disk: {} in {} worktree{}{unmeasured}.",
        super::triage::human_bytes(d.bytes),
        d.count,
        if d.count == 1 { "" } else { "s" },
    );
}

fn render_integration(out: &mut String, i: &Integration) {
    for h in &i.holds {
        let _ = writeln!(
            out,
            "Hold on dispatch of project {}: {}.",
            h.project, h.cause
        );
    }
    if i.bundle_refs > 0 {
        let _ = writeln!(out, "Bundle refs: {}.", i.bundle_refs);
    }
    if let Some(r) = &i.rescue {
        let _ = writeln!(
            out,
            "Rescue refs: {} unique ({}), {} superseded ({}).",
            r.unique.count,
            super::triage::human_bytes(r.unique.bytes),
            r.superseded.count,
            super::triage::human_bytes(r.superseded.bytes),
        );
    }
    for f in &i.features {
        let push = match f.push_state.as_deref() {
            None => "not pushed".to_string(),
            Some("failed") => format!("push failed {} times", f.push_failures),
            Some(state) => format!("push {state}"),
        };
        let name = f.name.as_deref().unwrap_or("no name");
        let _ = writeln!(out, "Feature {} ({name}) {}: {push}.", f.id, f.state);
    }
    for s in &i.stale_remotes {
        let _ = writeln!(
            out,
            "Remote tip of {} in {}: stale since {}.",
            s.branch, s.repo, s.stale_since
        );
    }
}

fn ago(s: i64) -> String {
    match s.max(0) {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

fn render_open_worktrees(out: &mut String, worktrees: &[WorktreeLine]) {
    let open: Vec<&WorktreeLine> = worktrees
        .iter()
        .filter(|w| {
            !WorktreeStatus::parse(&w.status).is_ok_and(|s| super::triage::SETTLED.contains(&s))
        })
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventLine {
    pub at: String,
    pub actor: String,
    pub kind: String,
    pub text: String,
}

pub fn events(conn: &Connection, column: &str, id: &str) -> Result<Vec<EventLine>> {
    let column = match column {
        "task_id" => "task_id",
        "run_id" => "run_id",
        _ => "worker_id",
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT at, actor, {kind}, {text} FROM events WHERE {column} = ?1 ORDER BY event_id",
        kind = super::util::shown_kind_sql(),
        text = super::util::shown_text_sql()
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

#[cfg(test)]
mod trf_15_tests {
    use std::path::Path;

    use super::*;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::NewTask;
    use crate::taskledger::Store;

    pub(crate) const CASES: &[(&str, bool, Option<&str>, bool, bool)] = &[
        ("open", true, None, false, false),
        ("landing", true, None, false, false),
        ("landed", true, None, false, false),
        ("landed", false, None, false, true),
        ("cleanup_failed", true, None, false, false),
        ("cleanup_failed", true, Some("ARCHIVE"), false, true),
        ("unlanded", true, None, false, false),
        ("unlanded", true, Some("KEEP"), false, true),
        ("unlanded", false, None, false, true),
        ("abandoned", true, None, false, false),
        ("abandoned", false, None, false, true),
        ("cleaned", false, None, false, true),
        ("archived", false, None, false, true),
        ("trashed", false, None, false, true),
        ("open", true, None, true, true),
    ];

    fn fixture(case: (&str, bool, Option<&str>, bool, bool)) -> (tempfile::TempDir, Store, Run) {
        let (status, on_disk, decision, scratch, _) = case;
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        store
            .conn()
            .execute_batch(
                "INSERT OR IGNORE INTO features (id, name, branch, primary_branch, state,
                    created_at, updated_at)
                 VALUES ('F-0001', 'r1', 'fleet/r1', 'main', 'open', 'now', 'now');
                 INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
                 VALUES ('I-0001', 'F-0001', 'in_progress', 'now', 'now');",
            )
            .unwrap();
        let t = NewTask::work("alpha", "tickets/alpha.md", "dev");
        let task = store
            .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
            .unwrap();
        let wt = dir.path().join("wt");
        if on_disk {
            std::fs::create_dir(&wt).unwrap();
        }
        let conn = store.conn();
        conn.execute(
            "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
            [&task.task_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base, status, decision, origin, created_at)
             VALUES ('wt1', ?1, '/repo', ?2, 'fleet/a', 'main', ?3, ?4, 'native', '2000-01-01T00:00:00Z')",
            params![
                (!scratch).then_some(&task.task_id),
                wt.to_string_lossy(),
                status,
                decision
            ],
        )
        .unwrap();
        if scratch {
            conn.execute(
                "INSERT INTO worktree_modes (worktree_id, run_id, name, scratch) VALUES ('wt1', ?1, 'wt', 1)",
                [&run.run_id],
            )
            .unwrap();
        }
        (dir, store, run)
    }

    #[test]
    fn trf_15_status_and_run_close_agree_on_every_worktree_state() {
        for &case in CASES {
            let (_d, store, run) = fixture(case);
            let settled = case.4;
            let items = unsettled(store.conn(), &run.run_id).unwrap();
            assert_eq!(items.is_empty(), settled, "{case:?}: {items:?}");
            let r = report(store.conn(), &run).unwrap();
            assert_eq!(r.complete, settled, "{case:?}");
            assert_eq!(r.unsettled, items, "{case:?}");
            assert_eq!(
                render(&r).contains("Unsettled: worktree "),
                !settled,
                "{case:?}"
            );
            let closed = runs::close_run(
                store.conn(),
                &crate::taskledger::git::Git::new("git"),
                &run,
                None,
            );
            assert_eq!(closed.is_ok(), settled, "{case:?}: {closed:?}");
        }
    }

    #[test]
    fn trf_15_an_open_or_held_task_keeps_the_run_unsettled() {
        let (_d, store, run) = fixture(("cleaned", false, None, false, true));
        store
            .conn()
            .execute(
                "UPDATE tasks SET status = 'escalated', held_reason = 'two failures'",
                [],
            )
            .unwrap();
        let items = unsettled(store.conn(), &run.run_id).unwrap();
        assert_eq!(
            items.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
            ["T-1 (held: two failures)"]
        );
        assert!(!report(store.conn(), &run).unwrap().complete);
        let err = runs::close_run(
            store.conn(),
            &crate::taskledger::git::Git::new("git"),
            &run,
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("T-1 (held: two failures)"),
            "{err}"
        );
    }
}

#[cfg(test)]
mod trf_39_tests {
    use std::path::Path;

    use super::*;
    use crate::taskledger::model::WorkerKind;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::workers::NewWorker;
    use crate::taskledger::Store;

    #[test]
    fn trf_39_a_pending_orchestrator_row_is_not_stale() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        for (kind, role) in [
            (WorkerKind::Orchestrator, "orchestrator"),
            (WorkerKind::Worker, "dev-1"),
        ] {
            store
                .immediate(|tx| {
                    workers::insert(
                        tx,
                        &NewWorker {
                            run_id: run.run_id.clone(),
                            kind: Some(kind),
                            persona: "opus".into(),
                            role: Some(role.into()),
                            ..Default::default()
                        },
                    )
                })
                .unwrap();
        }
        store
            .conn()
            .execute(
                "UPDATE workers SET status = 'spawning', last_checkin_at = NULL,
                 spawned_at = '2000-01-01T00:00:00Z'",
                [],
            )
            .unwrap();

        let r = report(store.conn(), &run).unwrap();
        let line = |role: &str| r.workers.iter().find(|w| w.worker.label() == role).unwrap();
        let orchestrator = line("orchestrator");
        assert!(!orchestrator.stale, "a pending orchestrator is not stale");
        assert!(orchestrator.launch_pending);
        let worker = line("dev-1");
        assert!(worker.stale, "a silent worker row stays stale");
        assert!(!worker.launch_pending);

        let text = render(&r);
        assert!(
            text.contains("Orchestrator orchestrator: launch pending."),
            "{text}"
        );
        assert!(text.contains("dev-1"), "{text}");
        assert_eq!(text.matches("STALE").count(), 1, "{text}");
    }
}

#[cfg(test)]
mod needs_you_tests {
    use std::path::Path;

    use super::*;
    use crate::fleetd::ideas::application::needs_you;
    use crate::fleetd::ideas::domain::needs_you::Write;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::Store;

    #[test]
    fn status_shows_each_open_needs_you_item() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        store
            .immediate(|tx| {
                needs_you::open_item(
                    tx,
                    &Write {
                        key: "illegal_transition:T-3",
                        kind: "illegal_transition",
                        entity: "task",
                        entity_id: "T-3",
                        text: "T-3 asked done -> running",
                    },
                )?;
                needs_you::open_item(
                    tx,
                    &Write {
                        key: "job_abandoned:J-1",
                        kind: "job_abandoned",
                        entity: "job",
                        entity_id: "J-1",
                        text: "J-1 ended abandoned 3 times",
                    },
                )?;
                needs_you::resolve(tx, "job_abandoned:J-1")
            })
            .unwrap();
        let report = report(store.conn(), &run).unwrap();
        assert_eq!(report.needs_you.open, 1);
        let keys: Vec<&str> = report
            .needs_you
            .items
            .iter()
            .map(|i| i.key.as_str())
            .collect();
        assert_eq!(keys, ["illegal_transition:T-3"]);

        let text = render(&report);
        assert!(text.contains("Needs you (1):"), "{text}");
        assert!(
            text.contains("illegal_transition:T-3: T-3 asked done -> running"),
            "{text}"
        );
        assert!(!text.contains("J-1"), "{text}");

        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["needs_you"]["open"], 1, "{json}");
        assert_eq!(
            json["needs_you"]["items"][0]["key"],
            "illegal_transition:T-3"
        );
    }
}
