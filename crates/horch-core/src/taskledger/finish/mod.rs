pub mod codesweep;
pub mod comments;
pub mod complexity;
pub mod detach;
pub mod land;
pub mod pr;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

use super::git::Git;
use super::model::{
    Actor, EventKind, HeldReason, StepName, StepStatus, TaskStatus, WorkerStatus, WorktreeStatus,
};
use super::runs::{self, Run};
use super::sql_words::{self, word};
use super::store::{optional, Store};
use super::tasks::{self, Task};
use super::util::{self, About};
use super::workers::{self, Worker};
use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::application::commands;
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeState,
};
use crate::fleetd::worktree_custody::application::create::{self, JobProcess, Lease, RunJob};
use crate::fleetd::worktree_custody::application::remove;

#[derive(Debug, Clone)]
pub struct Tools {
    pub git: Git,
    pub gh: PathBuf,
    pub node: PathBuf,
    pub complexity_cli: Option<PathBuf>,
    pub code_sweep_dir: Option<PathBuf>,
    pub evidence_root: PathBuf,
    pub shell: PathBuf,
    pub reviewers_down: Vec<(String, String)>,
    pub codex: CodexSandbox,
    pub jobs: Option<Arc<dyn RunJob>>,
}

impl std::fmt::Debug for dyn RunJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunJob")
    }
}

#[derive(Debug, Clone, Default)]
pub struct CodexSandbox {
    pub bin: PathBuf,
    pub state_root: PathBuf,
    pub personas: Vec<String>,
}

impl CodexSandbox {
    pub fn runs(&self, worker: &Worker) -> bool {
        worker.harness.as_deref() == Some("codex")
            || worker
                .persona
                .as_ref()
                .is_some_and(|p| self.personas.contains(p))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    pub seq: i64,
    pub name: String,
    pub status: String,
    pub rounds: i64,
    pub evidence_path: Option<String>,
    pub verdict: Option<String>,
    pub review_task_ids: Option<String>,
    pub judged_rev: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub status: StepStatus,
    pub verdict: Option<String>,
    pub evidence: Option<PathBuf>,
    pub note: String,
    pub judged_rev: Option<String>,
    pub review_task_ids: Option<Vec<String>>,
    pub stop_workers: Vec<String>,
}

impl Outcome {
    pub fn pass(verdict: &str, evidence: Option<PathBuf>, note: &str) -> Outcome {
        Outcome {
            status: StepStatus::Passed,
            verdict: Some(verdict.into()),
            evidence,
            note: note.into(),
            judged_rev: None,
            review_task_ids: None,
            stop_workers: Vec::new(),
        }
    }

    pub fn fail(verdict: &str, evidence: Option<PathBuf>, note: &str) -> Outcome {
        Outcome {
            status: StepStatus::Failed,
            ..Outcome::pass(verdict, evidence, note)
        }
    }

    pub fn not_applicable(note: &str) -> Outcome {
        Outcome {
            status: StepStatus::NotApplicable,
            ..Outcome::pass("not applicable", None, note)
        }
    }

    pub fn waiting(note: &str, reviews: Vec<String>) -> Outcome {
        Outcome {
            status: StepStatus::Running,
            review_task_ids: Some(reviews),
            ..Outcome::pass("waiting", None, note)
        }
    }

    pub fn judged(mut self, rev: Option<String>) -> Outcome {
        self.judged_rev = rev;
        self
    }
}

pub struct Ctx<'a> {
    pub tools: &'a Tools,
    pub run: &'a Run,
    pub task: &'a Task,
    pub worker: &'a Worker,
    pub worktree: Option<WorktreeRow>,
    pub dir: PathBuf,
    pub root: PathBuf,
    pub evidence_dir: PathBuf,
    pub seq: i64,
    pub default_check: Option<String>,
}

impl Ctx<'_> {
    pub fn diff_base(&self) -> Result<String> {
        if !self.task.uses_worktree() {
            if let Some(start) = shared_start(&self.tools.evidence_root, &self.task.task_id) {
                return Ok(start);
            }
        }
        self.run
            .run_branch
            .clone()
            .or(self.run.base_branch.clone())
            .context("the run has no run branch")
    }
}

pub fn default_check(conn: &Connection, run_id: &str) -> Result<Option<String>> {
    let check: Option<String> = conn
        .query_row(
            "SELECT default_check FROM runs WHERE run_id = ?1",
            [run_id],
            |r| r.get(0),
        )
        .with_context(|| format!("no run {run_id}"))?;
    Ok(check.filter(|c| !c.trim().is_empty()))
}

pub fn set_default_check(conn: &Connection, run_id: &str, check: &str) -> Result<()> {
    conn.execute(
        "UPDATE runs SET default_check = ?2 WHERE run_id = ?1",
        params![run_id, check],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorktreeRow {
    pub worktree_id: String,
    pub task_id: Option<String>,
    pub worker_id: Option<String>,
    pub repo_root: String,
    pub path: String,
    pub branch: String,
    pub base: String,
    pub base_commit: Option<String>,
    pub status: String,
    pub landed_commit: Option<String>,
    pub bytes_on_disk: Option<i64>,
    pub reason: Option<String>,
}

const WORKTREE_COLUMNS: &str = "worktree_id, task_id, worker_id, repo_root, path, branch, base, \
     base_commit, status, landed_commit, bytes_on_disk, reason";

impl WorktreeRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<WorktreeRow> {
        let (
            worktree_id,
            task_id,
            worker_id,
            repo_root,
            path,
            branch,
            base,
            base_commit,
            status,
            landed_commit,
            bytes_on_disk,
            reason,
        ) = r.try_into()?;
        Ok(WorktreeRow {
            worktree_id,
            task_id,
            worker_id,
            repo_root,
            path,
            branch,
            base,
            base_commit,
            status,
            landed_commit,
            bytes_on_disk,
            reason,
        })
    }
}

pub fn worktree_of(conn: &Connection, task_id: &str) -> Result<Option<WorktreeRow>> {
    optional(conn.query_row(
        &format!(
            "SELECT {WORKTREE_COLUMNS} FROM worktrees
             WHERE task_id = ?1 AND status NOT IN ({gone})
             ORDER BY created_at DESC, rowid DESC LIMIT 1",
            gone = sql_words::list(sql_words::WORKTREES_GONE)
        ),
        [task_id],
        WorktreeRow::from_row,
    ))
}

pub fn worktree(conn: &Connection, id: &str) -> Result<WorktreeRow> {
    conn.query_row(
        &format!("SELECT {WORKTREE_COLUMNS} FROM worktrees WHERE worktree_id = ?1"),
        [id],
        WorktreeRow::from_row,
    )
    .with_context(|| format!("no worktree {id}"))
}

pub fn worktrees_with(conn: &Connection, status: WorktreeStatus) -> Result<Vec<WorktreeRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {WORKTREE_COLUMNS} FROM worktrees WHERE status = ?1 ORDER BY created_at"
    ))?;
    let rows = stmt
        .query_map([status.as_str()], WorktreeRow::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub(crate) fn set_worktree(
    conn: &Connection,
    id: &str,
    status: WorktreeStatus,
    reason: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE worktrees SET status = ?2, reason = ?3 WHERE worktree_id = ?1",
        params![id, status.as_str(), reason],
    )?;
    Ok(())
}

pub fn task_dir(evidence_root: &Path, task_id: &str) -> PathBuf {
    evidence_root.join(task_id)
}

fn shared_start_path(evidence_root: &Path, task_id: &str) -> PathBuf {
    task_dir(evidence_root, task_id).join("shared-start")
}

pub(crate) fn shared_start(evidence_root: &Path, task_id: &str) -> Option<String> {
    std::fs::read_to_string(shared_start_path(evidence_root, task_id))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn shared_claim(tools: &Tools, run: &Run, task: &Task) -> Result<()> {
    if task.uses_worktree() || task.is_review() {
        return Ok(());
    }
    let Some(run_branch) = run.run_branch.as_deref() else {
        return Ok(());
    };
    let root = Path::new(&run.project_root);
    let on = tools.git.current_branch(root);
    if on.as_deref() != Some(run_branch) {
        bail!(
            "{} works in the shared checkout, and {} is on {}, not {run_branch}. \
             Ask the orchestrator to run this in {}: git switch {run_branch}",
            task.short(),
            root.display(),
            on.as_deref().unwrap_or("a detached HEAD"),
            root.display()
        );
    }
    let path = shared_start_path(&tools.evidence_root, &task.task_id);
    if !path.exists() {
        let head = tools.git.rev(root, "HEAD")?;
        std::fs::create_dir_all(path.parent().expect("a task folder"))?;
        std::fs::write(&path, format!("{head}\n"))?;
    }
    Ok(())
}

fn held_task(conn: &Connection, worker_id: &str) -> Result<(Worker, Task)> {
    let w = workers::get(conn, worker_id)?;
    let Some(t) = &w.task_id else {
        bail!("this worker holds no task: run horch ledger claim first");
    };
    let task = tasks::get(conn, t)?;
    if task.status == TaskStatus::Cancelled.as_str() {
        bail!(
            "{} was cancelled: {}. Stop work, then run horch done",
            task.short(),
            task.summary.as_deref().unwrap_or("no reason given")
        );
    }
    if task.current_worker_id.as_deref() != Some(worker_id) {
        bail!(
            "{} is no longer this worker's ({}); stop work and end your turn",
            task.short(),
            task.shown_status()
        );
    }
    Ok((w, task))
}

fn safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    s.trim_matches('-').chars().take(40).collect()
}

pub fn worktree_new(
    store: &mut Store,
    git: &Git,
    worker_id: &str,
    name: &str,
) -> Result<WorktreeRow> {
    let jobs = JobProcess::of_this_process()?;
    worktree_new_with(store, git, worker_id, name, &jobs)
}

pub fn worktree_new_with(
    store: &mut Store,
    git: &Git,
    worker_id: &str,
    name: &str,
    jobs: &dyn RunJob,
) -> Result<WorktreeRow> {
    let (_, task) = held_task(store.conn(), worker_id)?;
    if !task.uses_worktree() {
        bail!(
            "{} works in the shared checkout; it has no worktree",
            task.short()
        );
    }
    let new = match worktree_of(store.conn(), &task.task_id)? {
        Some(_) => None,
        None => Some(new_worktree(store, git, worker_id, &task, name)?),
    };
    let attempt = live_attempt(store.conn(), worker_id)?;
    let lease = Lease {
        task_id: Some(&task.task_id),
        worker_id,
        attempt: attempt.as_deref(),
        from: None,
        new: new.as_ref(),
    };
    let id = lease_tree(store, jobs, &lease)?;
    if let Some(new) = new.filter(|n| n.worktree_id == id) {
        made_event(store, &new, &task.run_id)?;
    }
    worktree(store.conn(), &id)
}

fn new_worktree(
    store: &Store,
    git: &Git,
    worker_id: &str,
    task: &Task,
    name: &str,
) -> Result<NewWorktree> {
    let run = runs::get(store.conn(), &task.run_id)?;
    let root = PathBuf::from(&run.project_root);
    let base = run
        .run_branch
        .clone()
        .or(run.base_branch.clone())
        .context("the run has no run branch: create runs in a git repository")?;
    let slug = format!("T{}-{}", task.seq, safe(name));
    let path = root.join(".worktrees").join(&slug);
    let branch = format!(
        "{}-T{}-{}",
        run.run_branch.as_deref().unwrap_or("fleet"),
        task.seq,
        safe(name)
    );
    if path.exists() {
        bail!(
            "{} exists but the ledger has no row for it; pick another name",
            path.display()
        );
    }
    let base_commit = git.rev(&root, &base)?;
    Ok(NewWorktree {
        worktree_id: util::new_id(),
        task_id: Some(task.task_id.clone()),
        worker_id: Some(worker_id.to_string()),
        repo_root: root.to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
        branch,
        base,
        base_commit: Some(base_commit),
        status: WorktreeStatus::Open,
        mode: None,
    })
}

pub(crate) fn live_attempt(conn: &Connection, worker_id: &str) -> Result<Option<String>> {
    optional(conn.query_row(
        "SELECT id FROM attempts WHERE worker_id = ?1 AND state = 'live'
         ORDER BY rowid DESC LIMIT 1",
        [worker_id],
        |r| r.get(0),
    ))
}

pub(crate) fn lease_tree(store: &Store, jobs: &dyn RunJob, lease: &Lease<'_>) -> Result<String> {
    lease_and_wait(store, jobs, lease)
}

pub fn lease_and_wait(store: &Store, jobs: &dyn RunJob, lease: &Lease<'_>) -> Result<String> {
    with_custody(store, |conn, ctx| {
        create::create_and_wait(conn, ctx, jobs, lease)
    })
}

pub(crate) fn remove_tree(
    store: &Store,
    jobs: &dyn RunJob,
    id: &str,
    cause: Cause<'_>,
    keep_branch: bool,
) -> Result<Result<(), String>> {
    with_custody(store, |conn, ctx| {
        remove::remove_tree(conn, ctx, jobs, id, cause, keep_branch)
    })
}

pub(crate) fn with_custody<T>(
    store: &Store,
    f: impl FnOnce(&mut Connection, &JobCtx<'_>) -> Result<T>,
) -> Result<T> {
    let config = create::fleet_config(store.path())?;
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let mut custody = store.second_writer()?;
    f(custody.conn_mut(), &ctx)
}

pub fn job_runner(tools: &Tools) -> Result<Arc<dyn RunJob>> {
    match &tools.jobs {
        Some(jobs) => Ok(Arc::clone(jobs)),
        None => Ok(Arc::new(JobProcess::of_this_process()?)),
    }
}

fn made_event(store: &mut Store, new: &NewWorktree, run_id: &str) -> Result<()> {
    store.immediate(|tx| {
        util::event(
            tx,
            &About {
                run: Some(run_id),
                task: new.task_id.as_deref(),
                worker: new.worker_id.as_deref(),
            },
            Actor::Worker,
            EventKind::Progress,
            &format!(
                "Worktree {} on branch {} from {}.",
                new.path, new.branch, new.base
            ),
        )?;
        Ok(())
    })
}

impl Step {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Step> {
        let (
            seq,
            name,
            status,
            rounds,
            evidence_path,
            verdict,
            review_task_ids,
            judged_rev,
            started_at,
            ended_at,
            note,
        ) = r.try_into()?;
        Ok(Step {
            seq,
            name,
            status,
            rounds,
            evidence_path,
            verdict,
            review_task_ids,
            judged_rev,
            started_at,
            ended_at,
            note,
        })
    }
}

pub fn steps_of(conn: &Connection, task_id: &str, worker_id: &str) -> Result<Vec<Step>> {
    let mut stmt = conn.prepare(
        "SELECT seq, name, status, rounds, evidence_path, verdict, review_task_ids, judged_rev,
            started_at, ended_at, note
         FROM task_steps WHERE task_id = ?1 AND worker_id = ?2 ORDER BY seq",
    )?;
    let rows = stmt
        .query_map(params![task_id, worker_id], Step::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn through(status: &str) -> bool {
    StepStatus::parse(status).is_ok_and(|s| s.is_through())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FinishReport {
    pub task: String,
    pub steps: Vec<Step>,
    pub complete: bool,
    pub stopped_at: Option<String>,
    pub message: String,
    pub queued_reviews: Vec<String>,
    pub superseded_workers: Vec<String>,
}

fn tree_of(git: &Git, dir: &Path) -> Option<String> {
    git.tree(dir).ok()
}

struct Attempt<'a> {
    tools: &'a Tools,
    run: Run,
    task: Task,
    w: Worker,
    names: Vec<StepName>,
    root: PathBuf,
    dir: PathBuf,
    evidence_dir: PathBuf,
    wt: Option<WorktreeRow>,
    default_check: Option<String>,
    land_seq: Option<i64>,
}

fn land_seq(names: &[StepName]) -> Option<i64> {
    names
        .iter()
        .position(|n| *n == StepName::Land)
        .map(|i| i as i64 + 1)
}

fn work_dir(wt: Option<&WorktreeRow>, root: &Path) -> PathBuf {
    wt.filter(|w| Path::new(&w.path).exists())
        .map(|w| PathBuf::from(&w.path))
        .unwrap_or_else(|| root.to_path_buf())
}

fn require_worktree(task: &Task, wt: Option<&WorktreeRow>) -> Result<()> {
    if task.uses_worktree() && wt.is_none() && !task.is_no_change_one_off() {
        bail!(
            "{} has no worktree. Make it with horch ledger worktree new <name>, work and commit there, then run finish",
            task.short()
        );
    }
    Ok(())
}

impl<'a> Attempt<'a> {
    fn open(store: &Store, tools: &'a Tools, worker_id: &str) -> Result<Attempt<'a>> {
        let (w, task) = held_task(store.conn(), worker_id)?;
        Attempt::of(store.conn(), tools, w, task)
    }

    fn of(conn: &Connection, tools: &'a Tools, w: Worker, task: Task) -> Result<Attempt<'a>> {
        if task.is_review() {
            bail!(
                "{} is a review task: it has no finish steps; run horch ledger done",
                task.short()
            );
        }
        let run = runs::get(conn, &task.run_id)?;
        let names = run.steps()?;
        let root = PathBuf::from(&run.project_root);
        let wt = worktree_of(conn, &task.task_id)?;
        require_worktree(&task, wt.as_ref())?;
        let dir = work_dir(wt.as_ref(), &root);
        let evidence_dir = tools.evidence_root.join(&task.task_id).join(&w.worker_id);
        std::fs::create_dir_all(&evidence_dir)?;
        let default_check = default_check(conn, &run.run_id)?;
        Ok(Attempt {
            tools,
            default_check,
            land_seq: land_seq(&names),
            run,
            task,
            w,
            names,
            root,
            dir,
            evidence_dir,
            wt,
        })
    }

    fn seed_steps(&self, store: &mut Store) -> Result<()> {
        store.immediate(|tx| {
            for (i, n) in self.names.iter().enumerate() {
                tx.execute(
                    &format!(
                        "INSERT OR IGNORE INTO task_steps (task_id, worker_id, seq, name, status)
                         VALUES (?1, ?2, ?3, ?4, {pending})",
                        pending = word(StepStatus::Pending)
                    ),
                    params![
                        self.task.task_id,
                        self.w.worker_id,
                        i as i64 + 1,
                        n.as_str()
                    ],
                )?;
            }
            let task_id = &self.task.task_id;
            if tasks::get(tx, task_id)?.status == TaskStatus::Running.as_str() {
                util::transition(
                    tx,
                    Entity::Task,
                    task_id,
                    TaskStatus::Verifying.as_str(),
                    Actor::Worker,
                    util::Why::Rule(
                        util::Rule::Finish,
                        &format!("{} starts to land", self.task.short()),
                    ),
                )?;
                tx.execute(
                    "UPDATE tasks SET updated_at = ?2 WHERE task_id = ?1",
                    params![task_id, util::now()],
                )?;
            }
            Ok(())
        })
    }

    fn reopen_after_land(&self, store: &mut Store) -> Result<()> {
        let (Some(w0), Some(rb), Some(ls)) =
            (&self.wt, self.run.run_branch.as_deref(), self.land_seq)
        else {
            return Ok(());
        };
        if !land::reopen_if_new_commits(store, &self.tools.git, &self.root, w0, rb)? {
            return Ok(());
        }
        store.immediate(|tx| {
            tx.execute(
                &format!(
                    "UPDATE task_steps SET status = {pending} WHERE task_id = ?1 AND worker_id = ?2 AND seq >= ?3",
                    pending = word(StepStatus::Pending)
                ),
                params![self.task.task_id, self.w.worker_id, ls],
            )?;
            Ok(())
        })
    }

    fn still_passes(&self, store: &Store, seq: i64) -> Result<bool> {
        let rows = steps_of(store.conn(), &self.task.task_id, &self.w.worker_id)?;
        let row = rows.iter().find(|r| r.seq == seq).expect("inserted above");
        if !StepStatus::parse(&row.status)?.is_through() {
            return Ok(false);
        }
        let frozen = self.frozen(&rows, seq);
        Ok(row.judged_rev.is_none()
            || frozen
            || row.judged_rev.as_deref() == tree_of(&self.tools.git, &self.dir).as_deref())
    }

    fn frozen(&self, rows: &[Step], seq: i64) -> bool {
        self.land_seq
            .is_some_and(|ls| seq < ls && rows.iter().any(|r| r.seq == ls && through(&r.status)))
    }

    fn run_and_record(&self, store: &mut Store, seq: i64, name: StepName) -> Result<Outcome> {
        let cx = self.ctx(store.conn(), seq)?;
        self.start(store, seq)?;
        let outcome = run_step(store, &cx, name)
            .unwrap_or_else(|e| Outcome::fail("error", None, &format!("{e:#}")));
        record(store, &self.task, &self.w, seq, name, &outcome)?;
        Ok(outcome)
    }

    fn ctx(&self, conn: &Connection, seq: i64) -> Result<Ctx<'_>> {
        Ok(Ctx {
            tools: self.tools,
            run: &self.run,
            task: &self.task,
            worker: &self.w,
            worktree: worktree_of(conn, &self.task.task_id)?,
            dir: self.dir.clone(),
            root: self.root.clone(),
            evidence_dir: self.evidence_dir.clone(),
            seq,
            default_check: self.default_check.clone(),
        })
    }

    fn start(&self, store: &mut Store, seq: i64) -> Result<()> {
        store.immediate(|tx| {
            tx.execute(
                &format!(
                    "UPDATE task_steps SET status = {running}, rounds = rounds + 1,
                        started_at = COALESCE(started_at, ?4)
                     WHERE task_id = ?1 AND worker_id = ?2 AND seq = ?3",
                    running = word(StepStatus::Running)
                ),
                params![self.task.task_id, self.w.worker_id, seq, util::now()],
            )?;
            Ok(())
        })
    }
}

pub struct VerifierAttempt<'a>(Attempt<'a>);

impl<'a> VerifierAttempt<'a> {
    pub fn open(
        conn: &Connection,
        tools: &'a Tools,
        task_id: &str,
        worker_id: &str,
    ) -> Result<VerifierAttempt<'a>> {
        let w = workers::get(conn, worker_id)?;
        let task = tasks::get(conn, task_id)?;
        Attempt::of(conn, tools, w, task).map(VerifierAttempt)
    }

    pub fn steps(&self) -> &[StepName] {
        &self.0.names
    }

    pub fn dir(&self) -> &Path {
        &self.0.dir
    }

    pub fn seed(&self, store: &mut Store) -> Result<()> {
        self.0.seed_steps(store)
    }

    pub fn row(&self, conn: &Connection, name: StepName) -> Result<Option<Step>> {
        let rows = steps_of(conn, &self.0.task.task_id, &self.0.w.worker_id)?;
        Ok(rows.into_iter().find(|r| r.name == name.as_str()))
    }

    pub fn stands(&self, store: &Store, name: StepName) -> Result<bool> {
        self.0.still_passes(store, self.seq(name)?)
    }

    pub fn run(&self, store: &mut Store, name: StepName) -> Result<Outcome> {
        self.0.run_and_record(store, self.seq(name)?, name)
    }

    pub fn start(&self, store: &mut Store, name: StepName) -> Result<()> {
        self.0.start(store, self.seq(name)?)
    }

    pub fn record(&self, store: &mut Store, name: StepName, o: &Outcome) -> Result<()> {
        record(store, &self.0.task, &self.0.w, self.seq(name)?, name, o)
    }

    pub fn complexity_sweep(
        &self,
        conn: &Connection,
        runner: &dyn complexity::ToolRunner,
    ) -> Result<Outcome> {
        let seq = self.seq(StepName::ComplexitySweep)?;
        complexity::step_with(&self.0.ctx(conn, seq)?, runner)
    }

    fn seq(&self, name: StepName) -> Result<i64> {
        self.0
            .names
            .iter()
            .position(|n| *n == name)
            .map(|i| i as i64 + 1)
            .with_context(|| format!("the run has no step {}", name.as_str()))
    }
}

#[derive(Default)]
struct Ran {
    stopped_at: Option<String>,
    message: String,
    queued_reviews: Vec<String>,
    superseded_workers: Vec<String>,
}

impl Ran {
    fn add(&mut self, task: &Task, name: StepName, o: Outcome) {
        if let Some(ids) = o.review_task_ids {
            self.queued_reviews = ids;
        }
        self.superseded_workers.extend(o.stop_workers);
        if !o.status.is_through() {
            self.stopped_at = Some(name.as_str().to_string());
            self.message = format!("{} {}: {}", task.short(), name.as_str(), o.note);
        }
    }

    fn report(self, store: &Store, task: &Task, worker_id: &str) -> Result<FinishReport> {
        let steps = steps_of(store.conn(), &task.task_id, worker_id)?;
        let complete = self.stopped_at.is_none() && steps.iter().all(|s| through(&s.status));
        let message = if complete {
            format!(
                "{}: every finish step passed. Run: horch ledger done \"<summary>\"",
                task.short()
            )
        } else {
            self.message
        };
        Ok(FinishReport {
            task: task.short(),
            steps,
            complete,
            stopped_at: self.stopped_at,
            message,
            queued_reviews: self.queued_reviews,
            superseded_workers: self.superseded_workers,
        })
    }
}

pub fn finish(store: &mut Store, tools: &Tools, worker_id: &str) -> Result<FinishReport> {
    let a = Attempt::open(store, tools, worker_id)?;
    a.seed_steps(store)?;
    a.reopen_after_land(store)?;
    let mut ran = Ran::default();
    for (seq, name) in (1..).zip(a.names.iter().copied()) {
        if ran.stopped_at.is_none() && !a.still_passes(store, seq)? {
            let outcome = a.run_and_record(store, seq, name)?;
            ran.add(&a.task, name, outcome);
        }
    }
    ran.report(store, &a.task, &a.w.worker_id)
}

pub fn rereview(store: &mut Store, tools: &Tools, worker_id: &str) -> Result<()> {
    let (w, task) = held_task(store.conn(), worker_id)?;
    codesweep::forget_tree(store.conn(), &tools.evidence_root, &task.task_id)?;
    store.immediate(|tx| {
        tx.execute(
            &format!(
                "UPDATE task_steps SET status = {pending} WHERE task_id = ?1 AND worker_id = ?2 AND name = 'code-sweep'",
                pending = word(StepStatus::Pending)
            ),
            params![task.task_id, w.worker_id],
        )?;
        Ok(())
    })
}

fn run_step(store: &mut Store, cx: &Ctx<'_>, name: StepName) -> Result<Outcome> {
    match name {
        StepName::Check | StepName::Recheck => land::check(cx, name),
        StepName::CommentSweep => comments::step(store, cx),
        StepName::CodeSweep => codesweep::step(store, cx),
        StepName::ComplexitySweep => complexity::step(cx),
        StepName::Land => land::land(store, cx),
        StepName::Pr => pr::step(store, cx),
        StepName::Cleanup => land::cleanup(store, cx),
    }
}

fn record(
    store: &mut Store,
    task: &Task,
    w: &Worker,
    seq: i64,
    name: StepName,
    o: &Outcome,
) -> Result<()> {
    store.immediate(|tx| {
        tx.execute(
            &format!(
                "UPDATE task_steps SET status = ?4, verdict = ?5, evidence_path = ?6, note = ?7,
                    judged_rev = ?8, review_task_ids = COALESCE(?9, review_task_ids),
                    ended_at = CASE WHEN ?4 IN ({ended}) THEN ?10 ELSE ended_at END
                 WHERE task_id = ?1 AND worker_id = ?2 AND seq = ?3",
                ended = sql_words::list(&[
                    StepStatus::Passed,
                    StepStatus::Failed,
                    StepStatus::NotApplicable
                ])
            ),
            params![
                task.task_id,
                w.worker_id,
                seq,
                o.status.as_str(),
                o.verdict,
                o.evidence.as_ref().map(|p| p.to_string_lossy().into_owned()),
                o.note,
                o.judged_rev,
                o.review_task_ids.as_ref().map(|v| util::json_list(v)),
                util::now()
            ],
        )?;
        let kind = match o.status {
            StepStatus::Passed | StepStatus::NotApplicable => EventKind::StepPassed,
            StepStatus::Failed => EventKind::StepFailed,
            _ => EventKind::StepStarted,
        };
        util::event(
            tx,
            &About {
                run: Some(&task.run_id),
                task: Some(&task.task_id),
                worker: Some(&w.worker_id),
            },
            Actor::Worker,
            kind,
            &format!(
                "{} {}: {} ({})",
                task.short(),
                name.as_str(),
                o.verdict.as_deref().unwrap_or(o.status.as_str()),
                o.note
            ),
        )?;
        tx.execute(
            "UPDATE workers SET last_progress_at = ?2, last_progress_source = 'agent', stall_rounds = 0
             WHERE worker_id = ?1",
            params![w.worker_id, util::now()],
        )?;
        Ok(())
    })
}

pub fn all_through(conn: &Connection, run: &Run, task_id: &str, worker_id: &str) -> Result<bool> {
    let names = run.steps()?;
    let steps = steps_of(conn, task_id, worker_id)?;
    Ok(names.iter().enumerate().all(|(i, _)| {
        steps
            .iter()
            .find(|s| s.seq == i as i64 + 1)
            .is_some_and(|s| through(&s.status))
    }))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Completed {
    pub task: String,
    pub task_id: String,
    pub run_id: String,
    pub review: bool,
    pub unlanded: bool,
    pub reviews_in_for: Option<(String, Option<String>)>,
    pub already: bool,
}

pub fn complete(
    store: &mut Store,
    worker_id: &str,
    summary: &str,
    unlanded: Option<&str>,
) -> Result<Completed> {
    if let Some(done) = already_completed(store.conn(), worker_id)? {
        return Ok(done);
    }
    let (_, task) = held_task(store.conn(), worker_id)?;
    check_completable(store.conn(), &task, worker_id, summary, unlanded)?;
    store.immediate(|tx| {
        let (status, text, kind) = final_state(tx, &task, summary, unlanded)?;
        mark_done(
            tx,
            &task,
            worker_id,
            (status, &text, kind),
            unlanded.is_some(),
        )?;
        Ok(Completed {
            task: task.short(),
            task_id: task.task_id.clone(),
            run_id: task.run_id.clone(),
            review: task.is_review(),
            unlanded: unlanded.is_some(),
            reviews_in_for: reviews_in_for(tx, &task)?,
            already: false,
        })
    })
}

pub fn already_completed(conn: &Connection, worker_id: &str) -> Result<Option<Completed>> {
    let w = workers::get(conn, worker_id)?;
    if w.status != "done" {
        return Ok(None);
    }
    let t = w
        .task_id
        .clone()
        .context("this worker finished without a task")?;
    let task = tasks::get(conn, &t)?;
    Ok(Some(Completed {
        task: task.short(),
        task_id: task.task_id.clone(),
        run_id: task.run_id.clone(),
        review: task.is_review(),
        unlanded: w.end_reason.as_deref() == Some("unlanded"),
        reviews_in_for: None,
        already: true,
    }))
}

fn check_completable(
    conn: &Connection,
    task: &Task,
    worker_id: &str,
    summary: &str,
    unlanded: Option<&str>,
) -> Result<()> {
    let run = runs::get(conn, &task.run_id)?;
    if summary.trim().is_empty() {
        bail!("give a summary: horch ledger done \"<what you did, files, decisions, gotchas>\"");
    }
    if task.is_review() {
        require_findings(task)
    } else if unlanded.is_some() {
        Ok(())
    } else {
        require_through(conn, &run, task, worker_id)
    }
}

fn require_findings(task: &Task) -> Result<()> {
    let findings = task.spec_ref.clone().unwrap_or_default();
    if findings.is_empty() || !Path::new(&findings).is_file() {
        bail!("write your findings file first: {findings}");
    }
    Ok(())
}

fn require_through(conn: &Connection, run: &Run, task: &Task, worker_id: &str) -> Result<()> {
    if all_through(conn, run, &task.task_id, worker_id)? {
        return Ok(());
    }
    let open: Vec<String> = steps_of(conn, &task.task_id, worker_id)?
        .iter()
        .filter(|s| !through(&s.status))
        .map(|s| format!("{} ({})", s.name, s.status))
        .collect();
    bail!(
        "{} is not finished: {}. Run horch ledger finish until every step passes. \
         Only if the work truly cannot land: horch ledger done --unlanded \"<reason>\" \"<summary>\"",
        task.short(),
        if open.is_empty() {
            "no finish step ran yet".to_string()
        } else {
            open.join(", ")
        }
    );
}

fn final_state(
    tx: &Connection,
    task: &Task,
    summary: &str,
    unlanded: Option<&str>,
) -> Result<(&'static str, String, EventKind)> {
    let Some(reason) = unlanded else {
        return Ok((
            TaskStatus::Done.as_str(),
            summary.to_string(),
            EventKind::Done,
        ));
    };
    if let Some(wt) = worktree_of(tx, &task.task_id)? {
        set_worktree(tx, &wt.worktree_id, WorktreeStatus::Unlanded, Some(reason))?;
        park_tree(tx, &wt.worktree_id, reason)?;
    }
    Ok((
        TaskStatus::Escalated.as_str(),
        format!("UNLANDED: {reason} | {summary}"),
        EventKind::Unlanded,
    ))
}

fn park_tree(tx: &Connection, id: &str, reason: &str) -> Result<()> {
    let Some(row) = trees::get(tx, id)? else {
        return Ok(());
    };
    trees::set_owner(tx, id, None)?;
    if row.state == WorktreeState::Active {
        let text = format!("unlanded: {reason}");
        let cause = Cause {
            actor: Actor::Worker,
            text: &text,
        };
        transition(
            tx,
            None,
            Entity::Worktree,
            id,
            "created",
            cause,
            &NeedsYouList,
        )?;
    }
    Ok(())
}

fn mark_done(
    tx: &Connection,
    task: &Task,
    worker_id: &str,
    (status, text, kind): (&str, &str, EventKind),
    unlanded: bool,
) -> Result<()> {
    let now = util::now();
    let current = tasks::get(tx, &task.task_id)?.status;
    if !sql_words::TASKS_IN_FLIGHT
        .iter()
        .any(|s| s.as_str() == current)
    {
        bail!(
            "{} is no longer running: stop work, then run horch done",
            task.short()
        );
    }
    let shown = format!(
        "{} {}: {text}",
        task.short(),
        if unlanded { "parked unlanded" } else { status }
    );
    util::transition(
        tx,
        Entity::Task,
        &task.task_id,
        status,
        Actor::Worker,
        util::Why::Event(kind, &shown),
    )?;
    if !unlanded {
        tasks::ready_dependents(tx, &task.task_id)?;
    }
    tx.execute(
        "UPDATE tasks SET summary = ?2, updated_at = ?3,
            finished_at = CASE WHEN ?4 IS NULL THEN ?3 END,
            held_reason = ?4,
            current_worker_id = CASE WHEN ?4 IS NULL THEN current_worker_id END
         WHERE task_id = ?1",
        params![
            task.task_id,
            text,
            now,
            unlanded.then_some(HeldReason::Unlanded.as_str())
        ],
    )?;
    let end_reason = if unlanded { "unlanded" } else { "done" };
    util::transition(
        tx,
        Entity::Worker,
        worker_id,
        WorkerStatus::Done.as_str(),
        Actor::Worker,
        util::Why::Rule(util::Rule::End, &format!("horch ledger done: {end_reason}")),
    )?;
    commands::end_own_attempt(tx, worker_id, WorkerStatus::Done.as_str())?;
    tx.execute(
        "UPDATE workers SET summary = ?2, end_reason = ?3, ended_at = ?4,
            last_progress_at = ?4
         WHERE worker_id = ?1",
        params![worker_id, text, end_reason, now],
    )?;
    Ok(())
}

pub fn open_reviews(conn: &Connection, parent: &str) -> Result<i64> {
    Ok(conn.query_row(
        &format!(
            "SELECT count(*) FROM tasks WHERE parent_task_id = ?1 AND kind = 'review'
               AND status NOT IN ({ended})",
            ended = sql_words::list(sql_words::TASKS_ENDED)
        ),
        [parent],
        |r| r.get(0),
    )?)
}

fn reviews_in_for(tx: &Connection, task: &Task) -> Result<Option<(String, Option<String>)>> {
    let Some(parent) = task.parent_task_id.as_deref().filter(|_| task.is_review()) else {
        return Ok(None);
    };
    if open_reviews(tx, parent)? > 0 {
        return Ok(None);
    }
    let p = tasks::get(tx, parent)?;
    let role = match &p.current_worker_id {
        Some(id) => workers::get(tx, id)?.role,
        None => None,
    };
    Ok(Some((p.short(), role)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::runs::NewRun;
    use crate::taskledger::workers::NewWorker;

    #[test]
    fn a_codex_worker_is_known_by_its_harness_or_its_persona() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let worker = |store: &mut Store, persona: &str, harness: Option<&str>| {
            store
                .immediate(|tx| {
                    let run = runs::create(
                        tx,
                        Path::new("/repo"),
                        &NewRun {
                            slug: format!("r-{persona}-{}", harness.unwrap_or("none")),
                            ..Default::default()
                        },
                    )?;
                    crate::taskledger::schema::run_idea(
                        tx,
                        &run.run_id,
                        &crate::taskledger::util::now(),
                        true,
                    )?;
                    let id = workers::insert(
                        tx,
                        &NewWorker {
                            run_id: run.run_id,
                            persona: persona.into(),
                            ..Default::default()
                        },
                    )?;
                    if let Some(h) = harness {
                        workers::set_launch(tx, &id, "r-1", h, "m", None)?;
                    }
                    workers::get(tx, &id)
                })
                .unwrap()
        };
        let codex = CodexSandbox {
            personas: vec!["codex-sol".into()],
            ..Default::default()
        };
        assert!(codex.runs(&worker(&mut store, "codex-sol", None)));
        assert!(codex.runs(&worker(&mut store, "custom", Some("codex"))));
        assert!(!codex.runs(&worker(&mut store, "opus", Some("claude"))));
        assert!(!codex.runs(&worker(&mut store, "opus", None)));
    }

    #[test]
    fn every_status_change_has_an_event_with_a_cause() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let (task_id, worker_id) = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    dir.path(),
                    &NewRun {
                        slug: "or-2".into(),
                        finish_steps: vec![StepName::Check],
                        ..Default::default()
                    },
                )?;
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                let task = tasks::add(
                    tx,
                    &run,
                    &tasks::NewTask {
                        check_none: true,
                        worktree_mode: crate::taskledger::model::WorktreeMode::Shared,
                        ..tasks::NewTask::work("One-off", tasks::ONE_OFF_TICKET, "opus")
                    },
                    Actor::Orchestrator,
                )?;
                let new = NewWorker {
                    run_id: run.run_id,
                    persona: "opus".into(),
                    ..Default::default()
                };
                let w = crate::taskledger::testing::bind_live(tx, &task.task_id, &new, None)?;
                Ok((task.task_id, w.worker_id))
            })
            .unwrap();
        let tools = Tools {
            git: Git::new("git"),
            gh: dir.path().join("gh"),
            node: "/bin/sh".into(),
            complexity_cli: None,
            code_sweep_dir: None,
            evidence_root: dir.path().join("evidence"),
            shell: "/bin/sh".into(),
            reviewers_down: Vec::new(),
            codex: CodexSandbox::default(),
            jobs: Some(std::sync::Arc::new(
                crate::fleetd::worktree_custody::application::create::TestJobs,
            )),
        };
        finish(&mut store, &tools, &worker_id).unwrap();
        complete(&mut store, &worker_id, "all done", None).unwrap();

        let moves = |entity: &str, id: &str| -> Vec<(Option<String>, String)> {
            let mut stmt = store
                .conn()
                .prepare(
                    "SELECT from_state, to_state, cause FROM events
                     WHERE entity = ?1 AND entity_id = ?2 ORDER BY rowid",
                )
                .unwrap();
            stmt.query_map([entity, id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<String>>(2)?))
            })
            .unwrap()
            .map(|r| {
                let (from, to, cause): (Option<String>, String, Option<String>) = r.unwrap();
                assert!(
                    cause.is_some_and(|c| !c.trim().is_empty()),
                    "{entity} {id}: {from:?} -> {to} has no cause"
                );
                (from, to)
            })
            .collect()
        };
        let expected_move =
            |from: Option<&str>, to: &str| (from.map(str::to_string), to.to_string());
        assert_eq!(
            moves("task", &task_id),
            vec![
                expected_move(Some("ready"), "leased"),
                expected_move(Some("leased"), "running"),
                expected_move(Some("running"), "verifying"),
                expected_move(Some("verifying"), "done"),
            ]
        );
        assert_eq!(
            moves("worker", &worker_id),
            vec![
                expected_move(Some("spawning"), "working"),
                expected_move(Some("working"), "done"),
            ]
        );
    }
}
