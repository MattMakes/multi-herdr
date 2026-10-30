//! Projects and runs (section 4.1, 4.2).

use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Row};
use serde::Serialize;

use super::model::{Actor, EventKind, Origin, RunStatus, StepName};
use super::store::optional;
use super::util::{self, About};
use crate::exeid::ExeIdentity;

/// 1 run: an orchestrator's body of work.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Run {
    pub run_id: String,
    pub project_id: String,
    pub project_root: String,
    pub slug: String,
    pub orchestrator_worker_id: Option<String>,
    pub workspace_id: Option<String>,
    pub base_branch: Option<String>,
    pub run_branch: Option<String>,
    pub remote: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub finish_steps: Vec<String>,
    pub check_every_s: i64,
    pub stall_rounds: i64,
    pub heartbeat_every_s: i64,
    pub heartbeat_stale_s: i64,
    pub timer_tolerance_s: i64,
    /// Attempts at 1 task before it is held for the operator (#168).
    pub max_attempts: i64,
    pub watchdog_pid: Option<i64>,
    pub watchdog_started: Option<i64>,
    pub watchdog_checkin_at: Option<String>,
    /// The horch version of the watchdog at its last checkin (#216).
    pub watchdog_version: Option<String>,
    /// The binary the watchdog runs, as it recorded at its last checkin.
    pub watchdog_exe: Option<ExeIdentity>,
    pub status: String,
    pub origin: String,
    pub started_at: String,
    pub ended_at: Option<String>,
}

impl Run {
    /// The run's finish steps, in order. A word this binary does not know is
    /// an error: a step must never be skipped by being unreadable.
    pub fn steps(&self) -> Result<Vec<StepName>> {
        self.finish_steps
            .iter()
            .map(|s| StepName::parse(s))
            .collect()
    }

    fn from_row(r: &Row<'_>) -> rusqlite::Result<Run> {
        Ok(Run {
            run_id: r.get("run_id")?,
            project_id: r.get("project_id")?,
            project_root: r.get("root")?,
            slug: r.get("slug")?,
            orchestrator_worker_id: r.get("orchestrator_worker_id")?,
            workspace_id: r.get("workspace_id")?,
            base_branch: r.get("base_branch")?,
            run_branch: r.get("run_branch")?,
            remote: r.get("remote")?,
            pr_number: r.get("pr_number")?,
            pr_url: r.get("pr_url")?,
            finish_steps: util::parse_json_list(
                r.get::<_, Option<String>>("finish_steps")?.as_deref(),
            ),
            check_every_s: r.get("check_every_s")?,
            stall_rounds: r.get("stall_rounds")?,
            heartbeat_every_s: r.get("heartbeat_every_s")?,
            heartbeat_stale_s: r.get("heartbeat_stale_s")?,
            timer_tolerance_s: r.get("timer_tolerance_s")?,
            max_attempts: r.get("max_attempts")?,
            watchdog_pid: r.get("watchdog_pid")?,
            watchdog_started: r.get("watchdog_started")?,
            watchdog_checkin_at: r.get("watchdog_checkin_at")?,
            watchdog_version: r.get("watchdog_version")?,
            watchdog_exe: r
                .get::<_, Option<String>>("watchdog_exe")?
                .and_then(|j| serde_json::from_str(&j).ok()),
            status: r.get("status")?,
            origin: r.get("origin")?,
            started_at: r.get("started_at")?,
            ended_at: r.get("ended_at")?,
        })
    }
}

const SELECT: &str =
    "SELECT r.*, p.root FROM runs r JOIN projects p ON p.project_id = r.project_id";

/// The 1 spelling a project root is stored and matched by: canonical when the
/// folder exists (macOS `/tmp` is `/private/tmp`), else as given.
pub fn root_key(root: &Path) -> String {
    std::fs::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// The project row for `root`, created when missing.
pub fn ensure_project(conn: &Connection, root: &Path) -> Result<String> {
    let root = root_key(root);
    if let Some(id) = optional(conn.query_row(
        "SELECT project_id FROM projects WHERE root = ?1",
        [&root],
        |r| r.get::<_, String>(0),
    ))? {
        return Ok(id);
    }
    let id = util::new_id();
    conn.execute(
        "INSERT INTO projects (project_id, root, created_at) VALUES (?1, ?2, ?3)",
        params![id, root, util::now()],
    )?;
    Ok(id)
}

/// What `horch ledger run new` records.
#[derive(Debug, Clone, Default)]
pub struct NewRun {
    pub slug: String,
    pub workspace_id: Option<String>,
    pub base_branch: Option<String>,
    pub run_branch: Option<String>,
    pub remote: Option<String>,
    /// Empty means the default pipeline.
    pub finish_steps: Vec<StepName>,
    /// Thresholds; `None` keeps the spec's default.
    pub thresholds: Thresholds,
}

/// The watchdog's and the supervisor's timing (section 4.2).
#[derive(Debug, Clone, Default)]
pub struct Thresholds {
    pub check_every_s: Option<i64>,
    pub stall_rounds: Option<i64>,
    pub heartbeat_every_s: Option<i64>,
    pub heartbeat_stale_s: Option<i64>,
    pub timer_tolerance_s: Option<i64>,
    pub max_attempts: Option<i64>,
}

/// Create a run in project `root`. Refuses a second active run with the
/// same slug in the project.
pub fn create(conn: &Connection, root: &Path, new: &NewRun) -> Result<Run> {
    if new.slug.trim().is_empty() {
        bail!("a run needs a slug");
    }
    if new.thresholds.max_attempts.is_some_and(|n| n < 1) {
        bail!("--max-attempts must be 1 or more");
    }
    let project = ensure_project(conn, root)?;
    let clash: Option<String> = optional(conn.query_row(
        "SELECT run_id FROM runs WHERE project_id = ?1 AND slug = ?2 AND status = 'active'",
        params![project, new.slug],
        |r| r.get(0),
    ))?;
    if let Some(id) = clash {
        bail!(
            "run '{}' is already active in this project ({id})",
            new.slug
        );
    }
    let steps = if new.finish_steps.is_empty() {
        super::model::default_finish_steps()
    } else {
        new.finish_steps.clone()
    };
    let words: Vec<String> = steps.iter().map(|s| s.as_str().to_string()).collect();
    let id = util::new_id();
    conn.execute(
        "INSERT INTO runs (run_id, project_id, slug, workspace_id, base_branch, run_branch,
            remote, finish_steps, status, origin, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            project,
            new.slug,
            new.workspace_id,
            new.base_branch,
            new.run_branch,
            new.remote,
            util::json_list(&words),
            RunStatus::Active.as_str(),
            Origin::Native.as_str(),
            util::now(),
        ],
    )?;
    let t = &new.thresholds;
    conn.execute(
        "UPDATE runs SET
            check_every_s = COALESCE(?2, check_every_s),
            stall_rounds = COALESCE(?3, stall_rounds),
            heartbeat_every_s = COALESCE(?4, heartbeat_every_s),
            heartbeat_stale_s = COALESCE(?5, heartbeat_stale_s),
            timer_tolerance_s = COALESCE(?6, timer_tolerance_s),
            max_attempts = COALESCE(?7, max_attempts)
         WHERE run_id = ?1",
        params![
            id,
            t.check_every_s,
            t.stall_rounds,
            t.heartbeat_every_s,
            t.heartbeat_stale_s,
            t.timer_tolerance_s,
            t.max_attempts
        ],
    )?;
    util::event(
        conn,
        &About {
            run: Some(&id),
            ..Default::default()
        },
        Actor::Orchestrator,
        EventKind::RunCreated,
        &format!(
            "Run {} created. Run branch: {}.",
            new.slug,
            new.run_branch.as_deref().unwrap_or("(none)")
        ),
    )?;
    get(conn, &id)
}

/// 1 run by id.
pub fn get(conn: &Connection, run_id: &str) -> Result<Run> {
    conn.query_row(
        &format!("{SELECT} WHERE r.run_id = ?1"),
        [run_id],
        Run::from_row,
    )
    .with_context(|| format!("no run {run_id}"))
}

/// Every run of the project at `root`, newest first.
pub fn of_project(conn: &Connection, root: &Path) -> Result<Vec<Run>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE p.root = ?1 ORDER BY r.started_at DESC, r.rowid DESC"
    ))?;
    let rows = stmt
        .query_map([root_key(root)], Run::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The run a command means: `explicit` (an id or a slug of this project),
/// else the 1 active run of the caller's herdr `workspace`, else the 1 active
/// run of the project. The second value is a `NOTE:` line when that 1 run
/// belongs to another workspace than the caller's (#89). 0 active runs, or
/// 2+ with none of them the workspace's, is an error that names them.
pub fn resolve(
    conn: &Connection,
    root: &Path,
    explicit: Option<&str>,
    workspace: Option<&str>,
) -> Result<(Run, Option<String>)> {
    let runs = of_project(conn, root)?;
    match explicit.filter(|k| !k.is_empty()) {
        Some(key) => by_key(conn, root, &runs, key).map(|r| (r, None)),
        None => the_active(&runs, root, workspace),
    }
}

/// The 1 active run a command means when it names none (see [`resolve`]).
fn the_active(runs: &[Run], root: &Path, workspace: Option<&str>) -> Result<(Run, Option<String>)> {
    let active: Vec<&Run> = runs
        .iter()
        .filter(|r| r.status == RunStatus::Active.as_str())
        .collect();
    let own: Vec<&Run> = active
        .iter()
        .copied()
        .filter(|r| workspace.is_some() && r.workspace_id.as_deref() == workspace)
        .collect();
    match (own.as_slice(), active.as_slice()) {
        ([one], _) => Ok(((*one).clone(), None)),
        ([], [one]) => Ok(((*one).clone(), other_workspace_note(one, workspace))),
        (_, []) => bail!(
            "no active run in {}. Create one: horch ledger run new <slug>",
            root.display()
        ),
        (mine, all) => bail!(
            "{}",
            ambiguous_runs(if mine.is_empty() { all } else { mine }, root)
        ),
    }
}

/// The `NOTE:` line for the 1 active run when it is not the caller's workspace's.
fn other_workspace_note(one: &Run, workspace: Option<&str>) -> Option<String> {
    workspace
        .filter(|ws| one.workspace_id.as_deref() != Some(*ws))
        .map(|ws| {
            format!(
                "NOTE: this command uses run {} of workspace {}, not of this workspace {ws}. Name a run with --run <slug>.",
                one.slug,
                one.workspace_id.as_deref().unwrap_or("(none)")
            )
        })
}

/// The error text when 2+ active runs fit and none is chosen.
fn ambiguous_runs(many: &[&Run], root: &Path) -> String {
    format!(
        "{} active runs in {}: {}. Name one with --run <slug>, or close one with horch ledger run close --run <slug>",
        many.len(),
        root.display(),
        many.iter()
            .map(|r| match &r.workspace_id {
                Some(ws) => format!("{} (workspace {ws})", r.slug),
                None => r.slug.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A run by id or slug: a slug picks its active run, else its newest. A run
/// id from another project (a worker in a worktree) still works.
fn by_key(conn: &Connection, root: &Path, runs: &[Run], key: &str) -> Result<Run> {
    runs.iter()
        .find(|r| r.run_id == key)
        .cloned()
        .or_else(|| by_slug(runs, key))
        .or_else(|| get(conn, key).ok())
        .with_context(|| format!("no run '{key}' in {}", root.display()))
}

/// The active run with slug `key`, else the newest with it.
fn by_slug(runs: &[Run], key: &str) -> Option<Run> {
    let same: Vec<&Run> = runs.iter().filter(|r| r.slug == key).collect();
    same.iter()
        .find(|r| r.status == RunStatus::Active.as_str())
        .or(same.first())
        .map(|r| (*r).clone())
}

/// Record the orchestrator's own worker row on the run.
pub fn set_orchestrator(conn: &Connection, run_id: &str, worker_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE runs SET orchestrator_worker_id = ?2 WHERE run_id = ?1",
        params![run_id, worker_id],
    )?;
    Ok(())
}

/// Record the run's pull request.
pub fn set_pr(conn: &Connection, run_id: &str, number: i64, url: &str) -> Result<()> {
    conn.execute(
        "UPDATE runs SET pr_number = ?2, pr_url = ?3 WHERE run_id = ?1",
        params![run_id, number, url],
    )?;
    Ok(())
}

/// Close a run: `done` or `abandoned`.
pub fn close(conn: &Connection, run_id: &str, status: RunStatus) -> Result<()> {
    conn.execute(
        "UPDATE runs SET status = ?2, ended_at = ?3 WHERE run_id = ?1",
        params![run_id, status.as_str(), util::now()],
    )?;
    Ok(())
}

/// What keeps a run open: each task that is queued (held included),
/// running or landing, and each worktree that is open or landing.
pub fn open_items(conn: &Connection, run_id: &str) -> Result<Vec<String>> {
    let mut out: Vec<String> = super::tasks::of_run(conn, run_id)?
        .into_iter()
        .filter(|t| matches!(t.status.as_str(), "queued" | "running" | "landing"))
        .map(|t| match &t.held_reason {
            Some(r) => format!("{} (held: {r})", t.short()),
            None => format!("{} ({})", t.short(), t.status),
        })
        .collect();
    out.extend(
        super::status::worktrees(conn, run_id)?
            .into_iter()
            .filter(|w| !w.scratch && matches!(w.status.as_str(), "open" | "landing"))
            .map(|w| format!("worktree {} ({})", w.path, w.status)),
    );
    Ok(out)
}

/// The statuses of a worktree that no step takes to an outcome.
const PARKED: &str = "'landed', 'cleanup_failed', 'unlanded', 'abandoned'";

/// The folders of the run's parked worktrees that are on disk, with their
/// decision (#308).
fn parked_on_disk(conn: &Connection, run_id: &str) -> Result<Vec<(String, Option<String>)>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT w.path, w.decision FROM worktrees w LEFT JOIN tasks t ON t.task_id = w.task_id
         LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
         WHERE (t.run_id = ?1 OR m.run_id = ?1) AND w.status IN ({PARKED})
         ORDER BY w.created_at, w.rowid"
    ))?;
    let rows = stmt
        .query_map([run_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<(String, Option<String>)>>>()?;
    Ok(rows
        .into_iter()
        .filter(|(p, _)| std::path::Path::new(p).exists())
        .collect())
}

/// The folders of the run's parked worktrees: on disk, not settled, not
/// held by a task that is still open.
pub fn parked_worktrees(conn: &Connection, run_id: &str) -> Result<Vec<String>> {
    Ok(parked_on_disk(conn, run_id)?
        .into_iter()
        .map(|p| p.0)
        .collect())
}

/// The parked worktrees of the run with no triage decision: they keep the
/// run open, with `--abandon` too (#308).
pub fn undecided_worktrees(conn: &Connection, run_id: &str) -> Result<Vec<String>> {
    Ok(parked_on_disk(conn, run_id)?
        .into_iter()
        .filter(|(_, d)| d.is_none())
        .map(|p| p.0)
        .collect())
}

/// Why `run` cannot close while `parked` have no decision.
pub fn parked_message(run: &Run, parked: &[String]) -> String {
    format!(
        "run {} has {} parked worktrees with no decision: {}. Run horch worktree triage, fill in its reply table, then run horch worktree triage apply <reply>",
        run.slug,
        parked.len(),
        parked.join(", ")
    )
}

/// What [`close_run`] did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Closed {
    pub status: String,
    /// The tasks `--abandon` cancelled, with the worker each one had.
    pub cancelled: Vec<(String, Option<String>)>,
    /// With `--abandon`: the parked worktrees with no decision. The cancels
    /// stand, the run stays active, and the caller refuses the close.
    pub parked: Vec<String>,
}

/// `horch ledger run close`: close an active run as `done`. It refuses while
/// [`open_items`] names anything, or a parked worktree has no triage
/// decision ([`undecided_worktrees`]). With `abandon`, every open task is
/// cancelled first (the 1 cancel path) and the run closes as `abandoned`;
/// an undecided worktree then keeps the run active, with the cancels done
/// ([`Closed::parked`]).
pub fn close_run(conn: &Connection, run: &Run, abandon: Option<&str>) -> Result<Closed> {
    if run.status != RunStatus::Active.as_str() {
        bail!("run {} is already {}", run.slug, run.status);
    }
    let mut cancelled = Vec::new();
    if let Some(reason) = abandon {
        for t in super::tasks::of_run(conn, &run.run_id)? {
            if matches!(t.status.as_str(), "queued" | "running" | "landing") {
                let holder = t.current_worker_id.clone();
                super::tasks::cancel(conn, &t, &format!("run abandoned: {reason}"))?;
                cancelled.push((t.short(), holder));
            }
        }
        // A worktree no task holds open any more.
        conn.execute(
            "UPDATE worktrees SET status = 'abandoned', reason = ?2
             WHERE status IN ('open', 'landing') AND task_id IN
                (SELECT task_id FROM tasks WHERE run_id = ?1)",
            params![run.run_id, format!("run abandoned: {reason}")],
        )?;
    }
    let open = open_items(conn, &run.run_id)?;
    if !open.is_empty() {
        bail!(
            "run {} still has open work: {}. Finish or cancel it, or run horch ledger run close --run {} --abandon \"<reason>\"",
            run.slug,
            open.join(", "),
            run.slug
        );
    }
    let parked = undecided_worktrees(conn, &run.run_id)?;
    if !parked.is_empty() {
        if abandon.is_none() {
            bail!("{}", parked_message(run, &parked));
        }
        return Ok(Closed {
            status: RunStatus::Active.as_str().to_string(),
            cancelled,
            parked,
        });
    }
    let status = if abandon.is_some() {
        RunStatus::Abandoned
    } else {
        RunStatus::Done
    };
    close(conn, &run.run_id, status)?;
    if let Some(orch) = &run.orchestrator_worker_id {
        super::workers::end(
            conn,
            orch,
            super::model::WorkerStatus::Done,
            "the run was closed",
            None,
        )?;
    }
    util::event(
        conn,
        &About {
            run: Some(&run.run_id),
            ..Default::default()
        },
        Actor::Orchestrator,
        EventKind::Note,
        &match abandon {
            Some(r) => format!("Run {} closed as abandoned: {r}", run.slug),
            None => format!("Run {} closed as done.", run.slug),
        },
    )?;
    Ok(Closed {
        status: status.as_str().to_string(),
        cancelled,
        parked: Vec::new(),
    })
}
