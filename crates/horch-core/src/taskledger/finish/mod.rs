//! The finish pipeline (section 5.5, #36 to #39): the ordered,
//! evidence-checked steps every attempt passes before `horch ledger done`.
//!
//! `horch ledger finish` runs the run's steps in order and stops at the
//! first that does not pass. A re-run starts again from the top: a step that
//! passed is kept while the branch's tree is the one it judged, and runs
//! again when a later fix changed it. Once `land` passed, the steps before
//! it are final, until a commit arrives that the run branch lacks.

pub mod codesweep;
pub mod complexity;
pub mod detach;
pub mod land;
pub mod pr;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

use super::git::Git;
use super::model::{Actor, EventKind, StepName, StepStatus, TaskStatus, WorktreeStatus};
use super::runs::{self, Run};
use super::store::{optional, Store};
use super::tasks::{self, Task};
use super::util::{self, About};
use super::workers::{self, Worker};

/// The programs and places the steps use.
#[derive(Debug, Clone)]
pub struct Tools {
    pub git: Git,
    /// `gh`, for the pr step.
    pub gh: PathBuf,
    /// `node`, for the complexity analyser.
    pub node: PathBuf,
    /// The complexity-sweep skill's analyser (`code/cli/dist/index.js`), or
    /// an executable that takes the same arguments.
    pub complexity_cli: Option<PathBuf>,
    /// The code-sweep skill's directory (it holds `reviewers/`).
    pub code_sweep_dir: Option<PathBuf>,
    /// Where horch writes step evidence (logs, reports).
    pub evidence_root: PathBuf,
    /// The shell that runs a check command.
    pub shell: PathBuf,
    /// Reviewer personas that cannot run on this host, with why (not in the
    /// roster, harness not installed). Code-sweep queues their fallback.
    pub reviewers_down: Vec<(String, String)>,
    /// What runs the check of a Codex worker inside its pane's sandbox.
    pub codex: CodexSandbox,
}

/// The check of a Codex worker runs in the sandbox of its pane, not with
/// the operator's full rights (#181): `finish` itself runs outside the
/// sandbox, and the check runs code the worker wrote.
#[derive(Debug, Clone, Default)]
pub struct CodexSandbox {
    /// The `codex` CLI.
    pub bin: PathBuf,
    /// The state root: the pane's 1 extra writable root, and the parent of
    /// its TMPDIR.
    pub state_root: PathBuf,
    /// The roster's Codex teammates. A worker with 1 of these personas is a
    /// Codex worker, also when the ledger has no harness for it.
    pub personas: Vec<String>,
}

impl CodexSandbox {
    /// Whether `worker` runs in a Codex pane.
    pub fn runs(&self, worker: &Worker) -> bool {
        worker.harness.as_deref() == Some("codex")
            || worker
                .persona
                .as_ref()
                .is_some_and(|p| self.personas.contains(p))
    }
}

/// 1 step row.
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

/// The result of 1 step run.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub status: StepStatus,
    pub verdict: Option<String>,
    pub evidence: Option<PathBuf>,
    pub note: String,
    /// The tree this result judged; a change re-runs the step.
    pub judged_rev: Option<String>,
    pub review_task_ids: Option<Vec<String>>,
    /// Reviewer workers whose review a new round superseded: stop them.
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

    /// The step waits for other work (reviewer tasks); `finish` stops here.
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

/// What 1 step needs to know.
pub struct Ctx<'a> {
    pub tools: &'a Tools,
    pub run: &'a Run,
    pub task: &'a Task,
    pub worker: &'a Worker,
    /// The worktree row, when the task uses one.
    pub worktree: Option<WorktreeRow>,
    /// Where the work is: the worktree, or the shared checkout.
    pub dir: PathBuf,
    /// The main checkout.
    pub root: PathBuf,
    pub evidence_dir: PathBuf,
    pub seq: i64,
}

/// 1 worktree row.
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

/// The columns [`WorktreeRow::from_row`] reads, in its field order.
const WORKTREE_COLUMNS: &str = "worktree_id, task_id, worker_id, repo_root, path, branch, base, \
     base_commit, status, landed_commit, bytes_on_disk, reason";

impl WorktreeRow {
    /// 1 row of `SELECT {WORKTREE_COLUMNS}`, read by position.
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

/// The newest worktree of a task that is not cleaned or abandoned.
pub fn worktree_of(conn: &Connection, task_id: &str) -> Result<Option<WorktreeRow>> {
    optional(conn.query_row(
        &format!(
            "SELECT {WORKTREE_COLUMNS} FROM worktrees
             WHERE task_id = ?1 AND status NOT IN ('cleaned', 'abandoned')
             ORDER BY created_at DESC, rowid DESC LIMIT 1"
        ),
        [task_id],
        WorktreeRow::from_row,
    ))
}

/// 1 worktree by id.
pub fn worktree(conn: &Connection, id: &str) -> Result<WorktreeRow> {
    conn.query_row(
        &format!("SELECT {WORKTREE_COLUMNS} FROM worktrees WHERE worktree_id = ?1"),
        [id],
        WorktreeRow::from_row,
    )
    .with_context(|| format!("no worktree {id}"))
}

/// Every worktree row with `status`.
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

/// The folder of 1 task's facts that outlive its workers (the start of a
/// shared task, the code-sweep round, waivers).
pub fn task_dir(evidence_root: &Path, task_id: &str) -> PathBuf {
    evidence_root.join(task_id)
}

/// The file that holds the `HEAD` a shared-checkout task started from.
fn shared_start_path(evidence_root: &Path, task_id: &str) -> PathBuf {
    task_dir(evidence_root, task_id).join("shared-start")
}

/// The commit a shared-checkout task started from, when it was recorded.
pub(crate) fn shared_start(evidence_root: &Path, task_id: &str) -> Option<String> {
    std::fs::read_to_string(shared_start_path(evidence_root, task_id))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The gate of a shared-checkout claim: the main checkout must be on the
/// run branch, so the worker's commits go there. The first claim records
/// `HEAD`; `land` later requires a commit after it. A worktree task or a
/// review passes at once.
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

/// The worker's current task, or an error a worker can act on.
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
            task.status
        );
    }
    Ok((w, task))
}

/// A branch-safe name.
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

/// `horch ledger worktree new <name>`: the worker's worktree, on a branch
/// from the run branch. A task that already has an open worktree gets it
/// back (an earlier attempt's work is kept).
pub fn worktree_new(
    store: &mut Store,
    git: &Git,
    worker_id: &str,
    name: &str,
) -> Result<WorktreeRow> {
    let (w, task) = held_task(store.conn(), worker_id)?;
    if !task.uses_worktree() {
        bail!(
            "{} works in the shared checkout; it has no worktree",
            task.short()
        );
    }
    match worktree_of(store.conn(), &task.task_id)? {
        Some(existing) => reuse_worktree(store, &existing, worker_id),
        None => create_worktree(store, git, &w, &task, name),
    }
}

/// The task's open worktree, now the worker's.
fn reuse_worktree(
    store: &mut Store,
    existing: &WorktreeRow,
    worker_id: &str,
) -> Result<WorktreeRow> {
    store.immediate(|tx| {
        tx.execute(
            "UPDATE worktrees SET worker_id = ?2 WHERE worktree_id = ?1",
            params![existing.worktree_id, worker_id],
        )?;
        Ok(())
    })?;
    worktree(store.conn(), &existing.worktree_id)
}

/// A new worktree for the task, on a new branch from the run branch.
fn create_worktree(
    store: &mut Store,
    git: &Git,
    w: &Worker,
    task: &Task,
    name: &str,
) -> Result<WorktreeRow> {
    let run = runs::get(store.conn(), &task.run_id)?;
    let root = PathBuf::from(&run.project_root);
    let base = run
        .run_branch
        .clone()
        .or(run.base_branch.clone())
        .context("the run has no run branch: create runs in a git repository")?;
    let slug = format!("T{}-{}", task.seq, safe(name));
    let path = root.join(".worktrees").join(&slug);
    // Beside the run branch, not under it: git cannot hold both
    // `fleet/x` and `fleet/x/T1-y`.
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
    let path_text = path.to_string_lossy().into_owned();
    git.run(
        &root,
        &["worktree", "add", "-b", &branch, &path_text, &base_commit],
    )?;
    let row = WorktreeRow {
        worktree_id: util::new_id(),
        task_id: Some(task.task_id.clone()),
        worker_id: Some(w.worker_id.clone()),
        repo_root: root.to_string_lossy().into_owned(),
        path: path_text,
        branch,
        base,
        base_commit: Some(base_commit),
        status: WorktreeStatus::Open.as_str().to_string(),
        landed_commit: None,
        bytes_on_disk: None,
        reason: None,
    };
    insert_worktree(store, &row, &task.run_id)
}

/// Record a new worktree of the task in `row.task_id`.
fn insert_worktree(store: &mut Store, row: &WorktreeRow, run_id: &str) -> Result<WorktreeRow> {
    store.immediate(|tx| {
        tx.execute(
            "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch, base,
                base_commit, status, origin, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'open', 'native', ?9)",
            params![
                row.worktree_id,
                row.task_id,
                row.worker_id,
                row.repo_root,
                row.path,
                row.branch,
                row.base,
                row.base_commit,
                util::now()
            ],
        )?;
        util::event(
            tx,
            &About {
                run: Some(run_id),
                task: row.task_id.as_deref(),
                worker: row.worker_id.as_deref(),
            },
            Actor::Worker,
            EventKind::Progress,
            &format!(
                "Worktree {} on branch {} from {}.",
                row.path, row.branch, row.base
            ),
        )?;
        Ok(())
    })?;
    worktree(store.conn(), &row.worktree_id)
}

impl Step {
    /// 1 row of the `task_steps` SELECT of [`steps_of`], read by position.
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

/// The steps of 1 attempt, in order.
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

/// Whether a step's status lets the pipeline go on: passed or not
/// applicable. An unknown status does not.
fn through(status: &str) -> bool {
    StepStatus::parse(status).is_ok_and(|s| s.is_through())
}

/// What `finish` did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FinishReport {
    pub task: String,
    pub steps: Vec<Step>,
    /// Every step passed or is not applicable: `horch ledger done` may run.
    pub complete: bool,
    /// The step that stopped the pipeline, and why.
    pub stopped_at: Option<String>,
    pub message: String,
    /// Reviewer tasks queued by code-sweep that no worker holds yet.
    pub queued_reviews: Vec<String>,
    /// Reviewer workers whose review a new round superseded: the caller
    /// stops them.
    pub superseded_workers: Vec<String>,
}

/// The tree the branch holds now: what a step judges.
fn tree_of(git: &Git, dir: &Path) -> Option<String> {
    git.tree(dir).ok()
}

/// 1 attempt's finish: the task, its worker and where its work is.
struct Attempt<'a> {
    tools: &'a Tools,
    run: Run,
    task: Task,
    w: Worker,
    names: Vec<StepName>,
    root: PathBuf,
    /// Where the work is: the worktree, or the shared checkout.
    dir: PathBuf,
    evidence_dir: PathBuf,
    wt: Option<WorktreeRow>,
    /// The seq of the land step, when the run has one.
    land_seq: Option<i64>,
}

/// The seq of the land step among `names`.
fn land_seq(names: &[StepName]) -> Option<i64> {
    names
        .iter()
        .position(|n| *n == StepName::Land)
        .map(|i| i as i64 + 1)
}

/// The work's folder: the worktree when it exists, else the checkout.
fn work_dir(wt: Option<&WorktreeRow>, root: &Path) -> PathBuf {
    wt.filter(|w| Path::new(&w.path).exists())
        .map(|w| PathBuf::from(&w.path))
        .unwrap_or_else(|| root.to_path_buf())
}

/// A task that uses a worktree finishes only once it has one.
fn require_worktree(task: &Task, wt: Option<&WorktreeRow>) -> Result<()> {
    if task.uses_worktree() && wt.is_none() {
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
        if task.is_review() {
            bail!(
                "{} is a review task: it has no finish steps; run horch ledger done",
                task.short()
            );
        }
        let run = runs::get(store.conn(), &task.run_id)?;
        let names = run.steps()?;
        let root = PathBuf::from(&run.project_root);
        let wt = worktree_of(store.conn(), &task.task_id)?;
        require_worktree(&task, wt.as_ref())?;
        let dir = work_dir(wt.as_ref(), &root);
        let evidence_dir = tools.evidence_root.join(&task.task_id).join(&w.worker_id);
        std::fs::create_dir_all(&evidence_dir)?;
        Ok(Attempt {
            tools,
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

    /// Make sure every step has its row; the task is landing now.
    fn seed_steps(&self, store: &mut Store) -> Result<()> {
        store.immediate(|tx| {
            for (i, n) in self.names.iter().enumerate() {
                tx.execute(
                    "INSERT OR IGNORE INTO task_steps (task_id, worker_id, seq, name, status)
                     VALUES (?1, ?2, ?3, ?4, 'pending')",
                    params![
                        self.task.task_id,
                        self.w.worker_id,
                        i as i64 + 1,
                        n.as_str()
                    ],
                )?;
            }
            if self.task.status == TaskStatus::Running.as_str() {
                tx.execute(
                    "UPDATE tasks SET status = 'landing', updated_at = ?2
                     WHERE task_id = ?1 AND status = 'running'",
                    params![self.task.task_id, util::now()],
                )?;
            }
            Ok(())
        })
    }

    /// Commits after a land: the steps from land on judge them again (#154).
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
                "UPDATE task_steps SET status = 'pending' WHERE task_id = ?1 AND worker_id = ?2 AND seq >= ?3",
                params![self.task.task_id, self.w.worker_id, ls],
            )?;
            Ok(())
        })
    }

    /// Whether step `seq` passed and still stands: while the tree it judged
    /// is the tree now, and always once the work landed.
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

    /// Whether step `seq` comes before the land and the land passed: then
    /// it is final.
    fn frozen(&self, rows: &[Step], seq: i64) -> bool {
        self.land_seq
            .is_some_and(|ls| seq < ls && rows.iter().any(|r| r.seq == ls && through(&r.status)))
    }

    /// Run step `seq` and record its outcome; an error fails the step.
    fn run_and_record(&self, store: &mut Store, seq: i64, name: StepName) -> Result<Outcome> {
        let cx = Ctx {
            tools: self.tools,
            run: &self.run,
            task: &self.task,
            worker: &self.w,
            worktree: worktree_of(store.conn(), &self.task.task_id)?,
            dir: self.dir.clone(),
            root: self.root.clone(),
            evidence_dir: self.evidence_dir.clone(),
            seq,
        };
        store.immediate(|tx| {
            tx.execute(
                "UPDATE task_steps SET status = 'running', rounds = rounds + 1,
                    started_at = COALESCE(started_at, ?4)
                 WHERE task_id = ?1 AND worker_id = ?2 AND seq = ?3",
                params![self.task.task_id, self.w.worker_id, seq, util::now()],
            )?;
            Ok(())
        })?;
        let outcome = run_step(store, &cx, name)
            .unwrap_or_else(|e| Outcome::fail("error", None, &format!("{e:#}")));
        record(store, &self.task, &self.w, seq, name, &outcome)?;
        Ok(outcome)
    }
}

/// What the steps that ran in 1 finish left for its report.
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

/// Run the finish steps of the worker's task. A step that still passes is
/// kept; the first step that does not pass stops the run.
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

/// `horch ledger finish --rereview`: the next finish queues a new
/// code-sweep round for the same tree; the open reviews of the current one
/// are cancelled as superseded.
pub fn rereview(store: &mut Store, tools: &Tools, worker_id: &str) -> Result<()> {
    let (w, task) = held_task(store.conn(), worker_id)?;
    codesweep::forget_tree(store.conn(), &tools.evidence_root, &task.task_id)?;
    store.immediate(|tx| {
        tx.execute(
            "UPDATE task_steps SET status = 'pending' WHERE task_id = ?1 AND worker_id = ?2 AND name = 'code-sweep'",
            params![task.task_id, w.worker_id],
        )?;
        Ok(())
    })
}

fn run_step(store: &mut Store, cx: &Ctx<'_>, name: StepName) -> Result<Outcome> {
    match name {
        StepName::Check | StepName::Recheck => land::check(cx, name),
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
            "UPDATE task_steps SET status = ?4, verdict = ?5, evidence_path = ?6, note = ?7,
                judged_rev = ?8, review_task_ids = COALESCE(?9, review_task_ids),
                ended_at = CASE WHEN ?4 IN ('passed', 'failed', 'not_applicable') THEN ?10 ELSE ended_at END
             WHERE task_id = ?1 AND worker_id = ?2 AND seq = ?3",
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
        // Progress: a finish step that ran is the worker advancing.
        tx.execute(
            "UPDATE workers SET last_progress_at = ?2, last_progress_source = 'agent', stall_rounds = 0
             WHERE worker_id = ?1",
            params![w.worker_id, util::now()],
        )?;
        Ok(())
    })
}

/// Whether every finish step of the attempt passed (or is not applicable).
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

/// What `horch ledger done` recorded.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Completed {
    pub task: String,
    pub task_id: String,
    pub run_id: String,
    pub review: bool,
    pub unlanded: bool,
    /// For a review task: the task it reviews, when every review of that
    /// task is now in, with the role of its worker to tell.
    pub reviews_in_for: Option<(String, Option<String>)>,
    /// Retried after a crash: nothing new was written.
    pub already: bool,
}

/// `horch ledger done`: finish the worker's task. A work task needs every
/// finish step through, unless `unlanded` parks it (its branch and worktree
/// stay). A review task needs its findings file.
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

/// A retry after a crash between the write and the report: what the first
/// `done` recorded, or `None` when the worker is not done.
fn already_completed(conn: &Connection, worker_id: &str) -> Result<Option<Completed>> {
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
        unlanded: false,
        reviews_in_for: None,
        already: true,
    }))
}

/// The preconditions of `done`: a summary; for a review, its findings
/// file; for work that lands, every finish step through.
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

/// The task's final status, summary and event. An unlanded task parks its
/// worktree.
fn final_state(
    tx: &Connection,
    task: &Task,
    summary: &str,
    unlanded: Option<&str>,
) -> Result<(&'static str, String, EventKind)> {
    let Some(reason) = unlanded else {
        return Ok(("done", summary.to_string(), EventKind::Done));
    };
    if let Some(wt) = worktree_of(tx, &task.task_id)? {
        set_worktree(tx, &wt.worktree_id, WorktreeStatus::Unlanded, Some(reason))?;
    }
    Ok((
        "cancelled",
        format!("UNLANDED: {reason} | {summary}"),
        EventKind::Unlanded,
    ))
}

/// Write the task's and the worker's final state, and its event.
fn mark_done(
    tx: &Connection,
    task: &Task,
    worker_id: &str,
    (status, text, kind): (&str, &str, EventKind),
    unlanded: bool,
) -> Result<()> {
    let now = util::now();
    // Never turn a task cancelled meanwhile into done (#136).
    let n = tx.execute(
        "UPDATE tasks SET status = ?2, summary = ?3, updated_at = ?4, finished_at = ?4
         WHERE task_id = ?1 AND status IN ('running', 'landing')",
        params![task.task_id, status, text, now],
    )?;
    if n == 0 {
        bail!(
            "{} is no longer running: stop work, then run horch done",
            task.short()
        );
    }
    tx.execute(
        "UPDATE workers SET status = 'done', summary = ?2, end_reason = ?3, ended_at = ?4,
            last_progress_at = ?4
         WHERE worker_id = ?1",
        params![
            worker_id,
            text,
            if unlanded { "unlanded" } else { "done" },
            now
        ],
    )?;
    util::event(
        tx,
        &About {
            run: Some(&task.run_id),
            task: Some(&task.task_id),
            worker: Some(worker_id),
        },
        Actor::Worker,
        kind,
        &format!("{} {}: {text}", task.short(), status),
    )
}

/// For a review task whose parent's reviews are now all in: the parent,
/// with the role of its worker to tell.
fn reviews_in_for(tx: &Connection, task: &Task) -> Result<Option<(String, Option<String>)>> {
    let Some(parent) = task.parent_task_id.as_deref().filter(|_| task.is_review()) else {
        return Ok(None);
    };
    let open: i64 = tx.query_row(
        "SELECT count(*) FROM tasks WHERE parent_task_id = ?1 AND kind = 'review'
           AND status NOT IN ('done', 'cancelled')",
        [parent],
        |r| r.get(0),
    )?;
    if open > 0 {
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

    /// #181: a worker's check runs in a Codex sandbox when the ledger
    /// records its harness as codex (a spawned worker), or when its persona
    /// is a Codex teammate (a worker the ledger has no harness for).
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
}
