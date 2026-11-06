//! Projects and runs (section 4.1, 4.2).

use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Row};
use serde::Serialize;

use super::model::{Actor, EventKind, Origin, RunStatus, StepName, WorktreeStatus};
use super::sql_words::{self, word};
use super::status::Unsettled;
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
        &format!(
            "SELECT run_id FROM runs WHERE project_id = ?1 AND slug = ?2 AND status = {active}",
            active = word(RunStatus::Active)
        ),
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

/// The slug prefix of a workspace's solo run, which holds its one-off tasks
/// (`horch ledger task`, TRF-42).
pub const SOLO_PREFIX: &str = "solo-";

/// The slug of the solo run of `workspace`.
pub fn solo_slug(workspace: &str) -> String {
    format!("{SOLO_PREFIX}{workspace}")
}

/// Whether `run` is a workspace's solo run.
pub fn is_solo(run: &Run) -> bool {
    run.slug.starts_with(SOLO_PREFIX)
}

/// What a new solo run of `workspace` records: its slug, its workspace and
/// the minimal profile's finish steps. The caller fills in the branches.
pub fn solo_new_run(workspace: &str) -> NewRun {
    NewRun {
        slug: solo_slug(workspace),
        workspace_id: Some(workspace.to_string()),
        finish_steps: super::model::VerifyProfile::Minimal.steps(),
        ..Default::default()
    }
}

/// The active solo run of `workspace` in project `root`, if any.
pub fn active_solo(conn: &Connection, root: &Path, workspace: &str) -> Result<Option<Run>> {
    let slug = solo_slug(workspace);
    Ok(of_project(conn, root)?
        .into_iter()
        .find(|r| r.slug == slug && r.status == RunStatus::Active.as_str()))
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
/// A solo run is chosen only when no other run fits: one-off tasks must not
/// make the workspace's planned run ambiguous (TRF-42).
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
    let (own, active) = (planned_first(own), planned_first(active));
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

/// `runs` without the solo runs when any other run is among them.
fn planned_first(mut runs: Vec<&Run>) -> Vec<&Run> {
    if runs.iter().any(|r| !is_solo(r)) {
        runs.retain(|r| !is_solo(r));
    }
    runs
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

/// The statuses of a worktree that no step takes to an outcome.
const PARKED: &[WorktreeStatus] = &[
    WorktreeStatus::Landed,
    WorktreeStatus::CleanupFailed,
    WorktreeStatus::Unlanded,
    WorktreeStatus::Abandoned,
];

/// The folders of the run's parked worktrees that are on disk, with their
/// decision (#308).
fn parked_on_disk(conn: &Connection, run_id: &str) -> Result<Vec<(String, Option<String>)>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT w.path, w.decision FROM worktrees w LEFT JOIN tasks t ON t.task_id = w.task_id
         LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
         WHERE (t.run_id = ?1 OR m.run_id = ?1) AND w.status IN ({parked})
         ORDER BY w.created_at, w.rowid",
        parked = sql_words::list(PARKED)
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
    /// `NOTE:` lines of the C5 checks: a gate or PR condition that is not
    /// applicable, or a declined PR.
    pub notes: Vec<String>,
}

/// `horch ledger run close`: close an active run as `done`. It refuses while
/// the run is not settled ([`super::status::unsettled`], C6). With `abandon`, every open task is
/// cancelled first (the 1 cancel path) and the run closes as `abandoned`;
/// an undecided worktree then keeps the run active, with the cancels done
/// ([`Closed::parked`]).
pub fn close_run(conn: &Connection, run: &Run, abandon: Option<&str>) -> Result<Closed> {
    close_inner(conn, run, abandon, None)
}

fn close_inner(
    conn: &Connection,
    run: &Run,
    abandon: Option<&str>,
    guards: Option<&CloseGuards>,
) -> Result<Closed> {
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
            &format!(
                "UPDATE worktrees SET status = {abandoned}, reason = ?2
                 WHERE status IN ({open}) AND task_id IN
                    (SELECT task_id FROM tasks WHERE run_id = ?1)",
                abandoned = word(WorktreeStatus::Abandoned),
                open = sql_words::list(sql_words::WORKTREES_OPEN)
            ),
            params![run.run_id, format!("run abandoned: {reason}")],
        )?;
    }
    // C6: the 1 settled rule. Run close adds nothing to it here.
    let (parked, open): (Vec<Unsettled>, Vec<Unsettled>) =
        super::status::unsettled(conn, &run.run_id)?
            .into_iter()
            .partition(|u| matches!(u, Unsettled::Parked { .. }));
    if !open.is_empty() {
        let open: Vec<String> = open.iter().map(|u| u.to_string()).collect();
        bail!(
            "run {} still has open work: {}. Finish or cancel it, or run horch ledger run close --run {} --abandon \"<reason>\"",
            run.slug,
            open.join(", "),
            run.slug
        );
    }
    let parked: Vec<String> = parked
        .into_iter()
        .filter_map(|u| match u {
            Unsettled::Parked { path, .. } => Some(path),
            _ => None,
        })
        .collect();
    if !parked.is_empty() {
        if abandon.is_none() {
            bail!("{}", parked_message(run, &parked));
        }
        return Ok(Closed {
            status: RunStatus::Active.as_str().to_string(),
            cancelled,
            parked,
            notes: Vec::new(),
        });
    }
    let mut notes = match (guards, abandon) {
        (Some(g), None) => {
            let notes = check_guards(conn, run, g)?;
            if let Some(reason) = &g.pr_declined {
                conn.execute(
                    "UPDATE runs SET pr_declined_reason = ?2 WHERE run_id = ?1",
                    params![run.run_id, reason],
                )?;
            }
            notes
        }
        _ => Vec::new(),
    };
    let status = if abandon.is_some() {
        RunStatus::Abandoned
    } else {
        RunStatus::Done
    };
    close(conn, &run.run_id, status)?;
    super::orchestrator::record_stop(conn, &run.run_id)?;
    notes.extend(remove_gate_worktree(conn, run)?);
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
        notes,
    })
}

/// 1 recorded run gate (C5): the run check on 1 run-branch tip.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gate {
    pub gate_id: i64,
    pub run_id: String,
    pub tip_sha: String,
    pub command: String,
    /// `None` when the check did not exit (it was killed by a signal).
    pub exit_code: Option<i64>,
    pub log_path: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

impl Gate {
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0)
    }

    fn from_row(r: &Row<'_>) -> rusqlite::Result<Gate> {
        Ok(Gate {
            gate_id: r.get("gate_id")?,
            run_id: r.get("run_id")?,
            tip_sha: r.get("tip_sha")?,
            command: r.get("command")?,
            exit_code: r.get("exit_code")?,
            log_path: r.get("log_path")?,
            started_at: r.get("started_at")?,
            ended_at: r.get("ended_at")?,
        })
    }
}

/// Write 1 `run_gates` row and an event; the row as stored.
pub fn record_gate(conn: &Connection, g: &Gate) -> Result<Gate> {
    conn.execute(
        "INSERT INTO run_gates (run_id, tip_sha, command, exit_code, log_path, started_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            g.run_id,
            g.tip_sha,
            g.command,
            g.exit_code,
            g.log_path,
            g.started_at,
            g.ended_at
        ],
    )?;
    let id = conn.last_insert_rowid();
    gate_worktree_row(conn, &get(conn, &g.run_id)?, &g.tip_sha)?;
    util::event(
        conn,
        &About {
            run: Some(&g.run_id),
            ..Default::default()
        },
        Actor::Orchestrator,
        EventKind::Note,
        &format!(
            "Run gate on {}: `{}` exit {}.",
            g.tip_sha,
            g.command,
            g.exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "none".into())
        ),
    )?;
    conn.query_row(
        "SELECT * FROM run_gates WHERE gate_id = ?1",
        [id],
        Gate::from_row,
    )
    .context("reading the run gate back")
}

/// The `worktree_modes` name of the gate folder of `run`.
fn gate_name(run: &Run) -> String {
    format!("_gate-{}", run.slug)
}

/// Record [`gate_dir`] as 1 open scratch worktree row of `run` at `tip`, so
/// `run close` removes it (TRF-19). The next gate reuses the row: it holds
/// the folder's path, which is unique. A row of an earlier run with the
/// same slug moves to `run`.
fn gate_worktree_row(conn: &Connection, run: &Run, tip: &str) -> Result<()> {
    let path = gate_dir(run).to_string_lossy().into_owned();
    let base = run.run_branch.as_deref().unwrap_or_default();
    let open = word(WorktreeStatus::Open);
    let found: Option<String> = optional(conn.query_row(
        "SELECT worktree_id FROM worktrees WHERE path = ?1",
        [&path],
        |r| r.get(0),
    ))?;
    let id = match found {
        Some(id) => {
            conn.execute(
                &format!(
                    "UPDATE worktrees SET base = ?2, base_commit = ?3, status = {open},
                        cleaned_at = NULL, reason = NULL WHERE worktree_id = ?1"
                ),
                params![id, base, tip],
            )?;
            id
        }
        None => {
            let id = util::new_id();
            conn.execute(
                &format!(
                    "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path,
                        branch, base, base_commit, status, origin, created_at)
                     VALUES (?1, NULL, NULL, ?2, ?3, '(detached)', ?4, ?5, {open}, 'native', ?6)"
                ),
                params![id, run.project_root, path, base, tip, util::now()],
            )?;
            id
        }
    };
    conn.execute(
        "INSERT INTO worktree_modes (worktree_id, run_id, name, scratch) VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(worktree_id) DO UPDATE SET run_id = ?2, name = ?3, scratch = 1",
        params![id, run.run_id, gate_name(run)],
    )?;
    Ok(())
}

/// Remove the open gate folder of `run` at its close: `git worktree remove`,
/// never forced. The row becomes `cleaned`. A `NOTE:` line when git refuses;
/// the folder and the open row then stay.
fn remove_gate_worktree(conn: &Connection, run: &Run) -> Result<Option<String>> {
    let found: Option<(String, String)> = optional(conn.query_row(
        &format!(
            "SELECT w.worktree_id, w.path FROM worktrees w
             JOIN worktree_modes m ON m.worktree_id = w.worktree_id
             WHERE m.run_id = ?1 AND m.name = ?2 AND m.scratch = 1 AND w.status = {open}",
            open = word(WorktreeStatus::Open)
        ),
        params![run.run_id, gate_name(run)],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    let Some((id, path)) = found else {
        return Ok(None);
    };
    if Path::new(&path).exists() {
        let git = super::git::Git::new("git");
        let out = git.try_run(Path::new(&run.project_root), &["worktree", "remove", &path])?;
        if !out.ok {
            return Ok(Some(format!(
                "NOTE: the gate folder {path} stays: git worktree remove refused: {}. Remove it yourself.",
                out.stderr.trim()
            )));
        }
    }
    conn.execute(
        &format!(
            "UPDATE worktrees SET status = {cleaned}, cleaned_at = ?2 WHERE worktree_id = ?1",
            cleaned = word(WorktreeStatus::Cleaned)
        ),
        params![id, util::now()],
    )?;
    Ok(None)
}

/// The newest gate of `run_id` on `tip`, passing or not.
pub fn last_gate(conn: &Connection, run_id: &str, tip: &str) -> Result<Option<Gate>> {
    optional(conn.query_row(
        "SELECT * FROM run_gates WHERE run_id = ?1 AND tip_sha = ?2
         ORDER BY gate_id DESC LIMIT 1",
        params![run_id, tip],
        Gate::from_row,
    ))
}

/// The folder the gate of `run` checks out its tip in: 1 detached worktree
/// per run, kept between gates so a build cache survives.
pub fn gate_dir(run: &Run) -> std::path::PathBuf {
    Path::new(&run.project_root)
        .join(".worktrees")
        .join(format!("_gate-{}", run.slug))
}

/// Check out `tip` in `dir`: a new detached worktree, or the old one moved
/// to `tip`. Ignored files, such as a build cache, stay. The ledger never
/// forces git, so a gate dir with changes is refused, with its files named.
fn checkout_tip(git: &super::git::Git, root: &Path, dir: &Path, tip: &str) -> Result<()> {
    if dir.join(".git").exists() {
        let dirty = git.dirty(dir)?;
        if !dirty.is_empty() {
            bail!(
                "the gate folder {} has changes: {}. A check must not leave changes. Remove them, then run horch ledger run gate again",
                dir.display(),
                dirty.join(", ")
            );
        }
        git.run(dir, &["checkout", "-q", "--detach", tip])?;
    } else {
        let d = dir.to_string_lossy();
        git.run(root, &["worktree", "add", "--detach", &d, tip])?;
    }
    Ok(())
}

/// `horch ledger run gate` (C5, TRF-19): run `command` with `shell` on the
/// tip of the run branch, in [`gate_dir`], with its output in a log under
/// `log_dir`. The gate as it ran; the caller records it ([`record_gate`]).
pub fn run_gate(
    git: &super::git::Git,
    shell: &Path,
    run: &Run,
    command: &str,
    log_dir: &Path,
) -> Result<Gate> {
    use std::io::Write;
    let branch = run.run_branch.as_deref().with_context(|| {
        format!(
            "run {} has no run branch, so it has no tip to gate",
            run.slug
        )
    })?;
    let root = Path::new(&run.project_root);
    let tip = git.rev(root, branch)?;
    let dir = gate_dir(run);
    checkout_tip(git, root, &dir, &tip)?;
    std::fs::create_dir_all(log_dir).with_context(|| format!("creating {}", log_dir.display()))?;
    let log = log_dir.join(format!(
        "gate-{}-{}.log",
        run.slug,
        &tip[..tip.len().min(12)]
    ));
    let mut file =
        std::fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;
    writeln!(file, "$ {command}\n# in {} at {tip}", dir.display())?;
    let started_at = util::now();
    let mut cmd = std::process::Command::new(shell);
    cmd.arg("-c")
        .arg(command)
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file.try_clone()?);
    crate::runtime::process::scrub_child_env(&mut cmd);
    let status = cmd
        .status()
        .with_context(|| format!("running the run check `{command}`"))?;
    writeln!(file, "\n# exit: {:?}", status.code())?;
    Ok(Gate {
        gate_id: 0,
        run_id: run.run_id.clone(),
        tip_sha: tip,
        command: command.to_string(),
        exit_code: status.code().map(i64::from),
        log_path: Some(log.to_string_lossy().into_owned()),
        started_at,
        ended_at: Some(util::now()),
    })
}

/// The state of the run's pull request, as `run close` needs it (C5).
#[derive(Debug, Clone, PartialEq)]
pub enum PrCheck {
    /// The run has no PR to wait for (no remote, no gh, no commit ahead);
    /// the text says why.
    NotApplicable(String),
    Merged(i64),
    /// The PR exists and is not merged: its number and gh's state word.
    Unmerged {
        number: i64,
        state: String,
    },
    /// The run has commits to bring to its main branch and no PR.
    Missing,
}

/// What `run close` checks after the settled rule (C5): the run-branch tip
/// (`None` when the run has no run branch) and the PR.
#[derive(Debug, Clone, PartialEq)]
pub struct CloseGuards {
    pub tip: Option<String>,
    pub pr: PrCheck,
    /// `--pr-declined "<reason>"`.
    pub pr_declined: Option<String>,
}

/// Refuse unless a passing gate exists for the tip and the PR is merged,
/// not applicable, or declined. The `NOTE:` lines for the caller.
fn check_guards(conn: &Connection, run: &Run, g: &CloseGuards) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    match &g.tip {
        None => notes.push(format!(
            "NOTE: run {} has no run branch, so the run gate is not applicable.",
            run.slug
        )),
        Some(tip) => match last_gate(conn, &run.run_id, tip)? {
            Some(gate) if gate.passed() => {}
            Some(gate) => bail!(
                "run {} has no passing gate on its tip {tip}: the last gate `{}` ended with exit {}; read {}. Fix the run branch, then run horch ledger run gate --run {}",
                run.slug,
                gate.command,
                gate.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "none".into()),
                gate.log_path.as_deref().unwrap_or("(no log)"),
                run.slug
            ),
            None => bail!(
                "run {} has no gate on its tip {tip}. Run horch ledger run gate --run {}",
                run.slug,
                run.slug
            ),
        },
    }
    let missing = match &g.pr {
        PrCheck::NotApplicable(why) => {
            notes.push(format!("NOTE: the PR condition is not applicable: {why}."));
            None
        }
        PrCheck::Merged(_) => None,
        PrCheck::Unmerged { number, state } => {
            Some(format!("its PR #{number} is {state}, not merged"))
        }
        PrCheck::Missing => Some("it has no PR".to_string()),
    };
    match (missing, &g.pr_declined) {
        (Some(why), None) => bail!(
            "run {} cannot close: {why}. Merge the PR, or run horch ledger run close --run {} --pr-declined \"<reason>\"",
            run.slug,
            run.slug
        ),
        (Some(_), Some(reason)) => {
            notes.push(format!("NOTE: the PR is declined: {reason}."))
        }
        (None, _) => {}
    }
    Ok(notes)
}

/// `horch ledger run close` with the C5 checks: the settled rule
/// ([`close_run`]), then for a close as `done` a passing gate on the tip and
/// a merged, not applicable or declined PR. `--abandon` skips the C5 checks.
pub fn close_run_with(
    conn: &Connection,
    run: &Run,
    abandon: Option<&str>,
    guards: &CloseGuards,
) -> Result<Closed> {
    close_inner(conn, run, abandon, Some(guards))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::git::Git;
    use crate::taskledger::store::Store;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(root: &Path, file: &str) -> String {
        std::fs::write(root.join(file), file).unwrap();
        git(root, &["add", file]);
        git(root, &["commit", "-qm", file]);
        git(root, &["rev-parse", "HEAD"])
    }

    /// A repository with `main` and the run branch `fleet/r1`, and run r1.
    fn world() -> (tempfile::TempDir, Store, Run) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        commit(&root, "a");
        git(&root, &["branch", "fleet/r1"]);
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some("fleet/r1".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        (dir, store, run)
    }

    fn gate(store: &mut Store, run: &Run, command: &str, logs: &Path) -> Gate {
        let ran = run_gate(&Git::new("git"), Path::new("/bin/sh"), run, command, logs).unwrap();
        store.immediate(|tx| record_gate(tx, &ran)).unwrap()
    }

    fn guards(tip: &str, pr: PrCheck, declined: Option<&str>) -> CloseGuards {
        CloseGuards {
            tip: Some(tip.to_string()),
            pr,
            pr_declined: declined.map(str::to_string),
        }
    }

    #[test]
    fn trf_19_run_gate_runs_the_check_on_the_run_branch_tip_and_records_it() {
        let (dir, mut store, run) = world();
        let root = Path::new(&run.project_root);
        // The run branch moves ahead of the checkout; the gate sees its tip.
        git(root, &["checkout", "-q", "fleet/r1"]);
        let tip = commit(root, "b");
        git(root, &["checkout", "-q", "main"]);
        let logs = dir.path().join("logs");
        let g = gate(&mut store, &run, "test -f b && echo gate-ran", &logs);
        assert_eq!(g.tip_sha, tip);
        assert_eq!(g.command, "test -f b && echo gate-ran");
        assert_eq!(g.exit_code, Some(0));
        assert!(g.passed());
        assert!(g.ended_at.is_some());
        let log = std::fs::read_to_string(g.log_path.as_deref().unwrap()).unwrap();
        assert!(log.contains("gate-ran"), "{log}");
        assert_eq!(
            last_gate(store.conn(), &run.run_id, &tip).unwrap(),
            Some(g.clone())
        );

        // A red check is recorded with its exit code; the gate dir is reused.
        let red = gate(&mut store, &run, "exit 3", &logs);
        assert_eq!(red.exit_code, Some(3));
        assert!(!red.passed());
        assert_eq!(
            last_gate(store.conn(), &run.run_id, &tip)
                .unwrap()
                .unwrap()
                .gate_id,
            red.gate_id
        );
        let rows: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM run_gates WHERE run_id = ?1",
                [&run.run_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 2);
    }

    #[test]
    fn trf_20_close_refuses_without_a_passing_gate_on_the_current_tip() {
        let (dir, mut store, run) = world();
        let root = Path::new(&run.project_root);
        let na = || PrCheck::NotApplicable("the run has no remote".into());
        let tip = git(root, &["rev-parse", "fleet/r1"]);
        let err = close_run_with(store.conn(), &run, None, &guards(&tip, na(), None)).unwrap_err();
        assert!(err.to_string().contains("has no gate on its tip"), "{err}");

        let logs = dir.path().join("logs");
        gate(&mut store, &run, "false", &logs);
        let err = close_run_with(store.conn(), &run, None, &guards(&tip, na(), None)).unwrap_err();
        assert!(err.to_string().contains("no passing gate"), "{err}");

        gate(&mut store, &run, "true", &logs);
        // The tip moves: the passing gate is for an old tip.
        git(root, &["checkout", "-q", "fleet/r1"]);
        let new_tip = commit(root, "c");
        let err =
            close_run_with(store.conn(), &run, None, &guards(&new_tip, na(), None)).unwrap_err();
        assert!(err.to_string().contains(&new_tip), "{err}");

        let closed = close_run_with(store.conn(), &run, None, &guards(&tip, na(), None)).unwrap();
        assert_eq!(closed.status, "done");
        assert_eq!(
            closed.notes,
            ["NOTE: the PR condition is not applicable: the run has no remote."]
        );
    }

    #[test]
    fn trf_20_close_refuses_while_the_pr_is_neither_merged_nor_declined() {
        let (dir, mut store, run) = world();
        let logs = dir.path().join("logs");
        let tip = gate(&mut store, &run, "true", &logs).tip_sha;
        let open = || PrCheck::Unmerged {
            number: 7,
            state: "open".into(),
        };
        let err =
            close_run_with(store.conn(), &run, None, &guards(&tip, open(), None)).unwrap_err();
        assert!(
            err.to_string().contains("PR #7 is open, not merged"),
            "{err}"
        );
        assert!(err.to_string().contains("--pr-declined"), "{err}");
        let err = close_run_with(
            store.conn(),
            &run,
            None,
            &guards(&tip, PrCheck::Missing, None),
        )
        .unwrap_err();
        assert!(err.to_string().contains("it has no PR"), "{err}");
        assert_eq!(get(store.conn(), &run.run_id).unwrap().status, "active");

        let merged = close_run_with(
            store.conn(),
            &run,
            None,
            &guards(&tip, PrCheck::Merged(7), None),
        );
        assert_eq!(merged.unwrap().status, "done");

        // Declined: closes and stores the reason.
        let (dir, mut store, run) = world();
        let tip = gate(&mut store, &run, "true", &dir.path().join("logs")).tip_sha;
        let closed = close_run_with(
            store.conn(),
            &run,
            None,
            &guards(&tip, open(), Some("superseded by #9")),
        )
        .unwrap();
        assert_eq!(closed.status, "done");
        let reason: Option<String> = store
            .conn()
            .query_row(
                "SELECT pr_declined_reason FROM runs WHERE run_id = ?1",
                [&run.run_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reason.as_deref(), Some("superseded by #9"));
    }

    /// The gate folder's worktree rows: (status, scratch, run_id).
    fn gate_rows(store: &Store, run: &Run) -> Vec<(String, i64, String)> {
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT w.status, m.scratch, m.run_id FROM worktrees w
                 JOIN worktree_modes m ON m.worktree_id = w.worktree_id WHERE w.path = ?1",
            )
            .unwrap();
        stmt.query_map([gate_dir(run).to_string_lossy()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    #[test]
    fn trf_19_the_gate_folder_is_1_scratch_row_reused_and_removed_at_close() {
        let (dir, mut store, run) = world();
        let logs = dir.path().join("logs");
        let tip = gate(&mut store, &run, "true", &logs).tip_sha;
        let open = || vec![("open".to_string(), 1, run.run_id.clone())];
        assert_eq!(gate_rows(&store, &run), open());
        gate(&mut store, &run, "true", &logs);
        assert_eq!(
            gate_rows(&store, &run),
            open(),
            "the next gate reuses the row"
        );
        assert!(gate_dir(&run).exists());
        assert!(super::super::status::unsettled(store.conn(), &run.run_id)
            .unwrap()
            .is_empty());

        let na = PrCheck::NotApplicable("the run has no remote".into());
        let closed = close_run_with(store.conn(), &run, None, &guards(&tip, na, None)).unwrap();
        assert_eq!(closed.status, "done");
        assert!(
            !gate_dir(&run).exists(),
            "run close removes the gate folder"
        );
        assert_eq!(
            gate_rows(&store, &run),
            vec![("cleaned".to_string(), 1, run.run_id.clone())]
        );
    }

    #[test]
    fn trf_20_abandon_skips_the_gate_and_pr_checks() {
        let (_dir, store, run) = world();
        let closed = close_run_with(
            store.conn(),
            &run,
            Some("stop"),
            &guards("0000", PrCheck::Missing, None),
        )
        .unwrap();
        assert_eq!(closed.status, "abandoned");
    }

    /// A ledger in a temp dir with a project folder `root`.
    fn solo_world() -> (tempfile::TempDir, Store, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        (dir, store, root)
    }

    #[test]
    fn trf_42_a_solo_run_uses_the_minimal_profile_and_its_workspace() {
        let new = solo_new_run("w7");
        assert_eq!(new.slug, "solo-w7");
        assert_eq!(new.workspace_id.as_deref(), Some("w7"));
        assert_eq!(
            new.finish_steps,
            crate::taskledger::model::VerifyProfile::Minimal.steps()
        );
    }

    #[test]
    fn trf_42_active_solo_finds_only_the_workspaces_active_solo_run() {
        let (_dir, mut store, root) = solo_world();
        assert!(active_solo(store.conn(), &root, "w7").unwrap().is_none());
        let other = store
            .immediate(|tx| create(tx, &root, &solo_new_run("w8")))
            .unwrap();
        assert!(active_solo(store.conn(), &root, "w7").unwrap().is_none());
        let mine = store
            .immediate(|tx| create(tx, &root, &solo_new_run("w7")))
            .unwrap();
        let found = active_solo(store.conn(), &root, "w7").unwrap().unwrap();
        assert_eq!(found.run_id, mine.run_id);
        assert_ne!(found.run_id, other.run_id);
        store
            .immediate(|tx| close(tx, &mine.run_id, RunStatus::Done))
            .unwrap();
        assert!(active_solo(store.conn(), &root, "w7").unwrap().is_none());
    }

    #[test]
    fn trf_42_a_solo_run_never_makes_the_planned_run_ambiguous() {
        let (_dir, mut store, root) = solo_world();
        let planned = NewRun {
            slug: "plan".into(),
            workspace_id: Some("w7".into()),
            ..Default::default()
        };
        store.immediate(|tx| create(tx, &root, &planned)).unwrap();
        store
            .immediate(|tx| create(tx, &root, &solo_new_run("w7")))
            .unwrap();
        let (run, note) = resolve(store.conn(), &root, None, Some("w7")).unwrap();
        assert_eq!((run.slug.as_str(), note), ("plan", None));
        let (run, _) = resolve(store.conn(), &root, Some("solo-w7"), Some("w7")).unwrap();
        assert_eq!(run.slug, "solo-w7");
        // The caller's own solo run beats another workspace's planned run.
        store
            .immediate(|tx| create(tx, &root, &solo_new_run("w9")))
            .unwrap();
        let (run, note) = resolve(store.conn(), &root, None, Some("w9")).unwrap();
        assert_eq!((run.slug.as_str(), note), ("solo-w9", None));
    }
}
