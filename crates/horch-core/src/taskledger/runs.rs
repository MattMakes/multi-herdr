//! Projects and runs (section 4.1, 4.2).

use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Row};
use serde::Serialize;

use super::model::{Actor, EventKind, Origin, RunStatus, StepName};
use super::store::optional;
use super::util::{self, About};

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
    pub watchdog_pid: Option<i64>,
    pub watchdog_started: Option<i64>,
    pub watchdog_checkin_at: Option<String>,
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
            watchdog_pid: r.get("watchdog_pid")?,
            watchdog_started: r.get("watchdog_started")?,
            watchdog_checkin_at: r.get("watchdog_checkin_at")?,
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
}

/// Create a run in project `root`. Refuses a second active run with the
/// same slug in the project.
pub fn create(conn: &Connection, root: &Path, new: &NewRun) -> Result<Run> {
    if new.slug.trim().is_empty() {
        bail!("a run needs a slug");
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
            timer_tolerance_s = COALESCE(?6, timer_tolerance_s)
         WHERE run_id = ?1",
        params![
            id,
            t.check_every_s,
            t.stall_rounds,
            t.heartbeat_every_s,
            t.heartbeat_stale_s,
            t.timer_tolerance_s
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
/// else the 1 active run of the project. 0 or 2+ active runs without
/// `explicit` is an error that names them.
pub fn resolve(conn: &Connection, root: &Path, explicit: Option<&str>) -> Result<Run> {
    let runs = of_project(conn, root)?;
    if let Some(key) = explicit.filter(|k| !k.is_empty()) {
        if let Some(run) = runs.iter().find(|r| r.run_id == key) {
            return Ok(run.clone());
        }
        // A slug picks the active run with it, else the newest.
        let by_slug: Vec<&Run> = runs.iter().filter(|r| r.slug == key).collect();
        if let Some(run) = by_slug
            .iter()
            .find(|r| r.status == RunStatus::Active.as_str())
            .or(by_slug.first())
        {
            return Ok((*run).clone());
        }
        // A run id from another project (a worker in a worktree) still works.
        if let Ok(run) = get(conn, key) {
            return Ok(run);
        }
        bail!("no run '{key}' in {}", root.display());
    }
    let active: Vec<&Run> = runs
        .iter()
        .filter(|r| r.status == RunStatus::Active.as_str())
        .collect();
    match active.as_slice() {
        [one] => Ok((*one).clone()),
        [] => bail!(
            "no active run in {}. Create one: horch ledger run new <slug>",
            root.display()
        ),
        many => bail!(
            "{} active runs in {}: {}. Name one with --run <slug>",
            many.len(),
            root.display(),
            many.iter()
                .map(|r| r.slug.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
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
