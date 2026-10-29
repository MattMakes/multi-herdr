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

impl WorktreeRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<WorktreeRow> {
        Ok(WorktreeRow {
            worktree_id: r.get("worktree_id")?,
            task_id: r.get("task_id")?,
            worker_id: r.get("worker_id")?,
            repo_root: r.get("repo_root")?,
            path: r.get("path")?,
            branch: r.get("branch")?,
            base: r.get("base")?,
            base_commit: r.get("base_commit")?,
            status: r.get("status")?,
            landed_commit: r.get("landed_commit")?,
            bytes_on_disk: r.get("bytes_on_disk")?,
            reason: r.get("reason")?,
        })
    }
}

/// The newest worktree of a task that is not cleaned or abandoned.
pub fn worktree_of(conn: &Connection, task_id: &str) -> Result<Option<WorktreeRow>> {
    optional(conn.query_row(
        "SELECT * FROM worktrees WHERE task_id = ?1 AND status NOT IN ('cleaned', 'abandoned')
         ORDER BY created_at DESC, rowid DESC LIMIT 1",
        [task_id],
        WorktreeRow::from_row,
    ))
}

/// 1 worktree by id.
pub fn worktree(conn: &Connection, id: &str) -> Result<WorktreeRow> {
    conn.query_row(
        "SELECT * FROM worktrees WHERE worktree_id = ?1",
        [id],
        WorktreeRow::from_row,
    )
    .with_context(|| format!("no worktree {id}"))
}

/// Every worktree row with `status`.
pub fn worktrees_with(conn: &Connection, status: WorktreeStatus) -> Result<Vec<WorktreeRow>> {
    let mut stmt = conn.prepare("SELECT * FROM worktrees WHERE status = ?1 ORDER BY created_at")?;
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
    if let Some(existing) = worktree_of(store.conn(), &task.task_id)? {
        store.immediate(|tx| {
            tx.execute(
                "UPDATE worktrees SET worker_id = ?2 WHERE worktree_id = ?1",
                params![existing.worktree_id, worker_id],
            )?;
            Ok(())
        })?;
        return worktree(store.conn(), &existing.worktree_id);
    }
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
    let id = util::new_id();
    store.immediate(|tx| {
        tx.execute(
            "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch, base,
                base_commit, status, origin, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'open', 'native', ?9)",
            params![
                id,
                task.task_id,
                worker_id,
                root.to_string_lossy(),
                path_text,
                branch,
                base,
                base_commit,
                util::now()
            ],
        )?;
        util::event(
            tx,
            &About {
                run: Some(&task.run_id),
                task: Some(&task.task_id),
                worker: Some(&w.worker_id),
            },
            Actor::Worker,
            EventKind::Progress,
            &format!("Worktree {path_text} on branch {branch} from {base}."),
        )?;
        Ok(())
    })?;
    worktree(store.conn(), &id)
}

/// The steps of 1 attempt, in order.
pub fn steps_of(conn: &Connection, task_id: &str, worker_id: &str) -> Result<Vec<Step>> {
    let mut stmt = conn.prepare(
        "SELECT seq, name, status, rounds, evidence_path, verdict, review_task_ids, judged_rev,
            started_at, ended_at, note
         FROM task_steps WHERE task_id = ?1 AND worker_id = ?2 ORDER BY seq",
    )?;
    let rows = stmt
        .query_map(params![task_id, worker_id], |r| {
            Ok(Step {
                seq: r.get(0)?,
                name: r.get(1)?,
                status: r.get(2)?,
                rounds: r.get(3)?,
                evidence_path: r.get(4)?,
                verdict: r.get(5)?,
                review_task_ids: r.get(6)?,
                judged_rev: r.get(7)?,
                started_at: r.get(8)?,
                ended_at: r.get(9)?,
                note: r.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
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
}

/// The tree the branch holds now: what a step judges.
fn tree_of(git: &Git, dir: &Path) -> Option<String> {
    git.tree(dir).ok()
}

/// Run the finish steps of the worker's task.
pub fn finish(store: &mut Store, tools: &Tools, worker_id: &str) -> Result<FinishReport> {
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
    if task.uses_worktree() && wt.is_none() {
        bail!(
            "{} has no worktree. Make it with horch ledger worktree new <name>, work and commit there, then run finish",
            task.short()
        );
    }
    let dir = wt
        .as_ref()
        .filter(|w| Path::new(&w.path).exists())
        .map(|w| PathBuf::from(&w.path))
        .unwrap_or_else(|| root.clone());
    let evidence_dir = tools.evidence_root.join(&task.task_id).join(&w.worker_id);
    std::fs::create_dir_all(&evidence_dir)?;
    // Make sure every step has its row.
    store.immediate(|tx| {
        for (i, n) in names.iter().enumerate() {
            tx.execute(
                "INSERT OR IGNORE INTO task_steps (task_id, worker_id, seq, name, status)
                 VALUES (?1, ?2, ?3, ?4, 'pending')",
                params![task.task_id, w.worker_id, i as i64 + 1, n.as_str()],
            )?;
        }
        if task.status == TaskStatus::Running.as_str() {
            tx.execute(
                "UPDATE tasks SET status = 'landing', updated_at = ?2 WHERE task_id = ?1",
                params![task.task_id, util::now()],
            )?;
        }
        Ok(())
    })?;
    let about = |s: &str| format!("{} {s}", task.short());
    let land_seq = names
        .iter()
        .position(|n| *n == StepName::Land)
        .map(|i| i as i64 + 1);
    // Commits after a land: the steps before land judge them again (#154).
    if let (Some(w0), Some(rb), Some(ls)) = (&wt, run.run_branch.as_deref(), land_seq) {
        if land::reopen_if_new_commits(store, &tools.git, &root, w0, rb)? {
            store.immediate(|tx| {
                tx.execute(
                    "UPDATE task_steps SET status = 'pending' WHERE task_id = ?1 AND worker_id = ?2 AND seq >= ?3",
                    params![task.task_id, w.worker_id, ls],
                )?;
                Ok(())
            })?;
        }
    }
    let mut stopped_at = None;
    let mut message = String::new();
    let mut queued_reviews = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let seq = i as i64 + 1;
        let rows = steps_of(store.conn(), &task.task_id, &w.worker_id)?;
        let row = rows
            .iter()
            .find(|r| r.seq == seq)
            .cloned()
            .expect("inserted above");
        let land_passed = land_seq.is_some_and(|ls| {
            rows.iter()
                .any(|r| r.seq == ls && StepStatus::parse(&r.status).is_ok_and(|s| s.is_through()))
        });
        let through = StepStatus::parse(&row.status)?.is_through();
        let before_land = land_seq.is_some_and(|ls| seq < ls);
        // A passed step stands while the tree it judged is the tree now,
        // and always once the work landed.
        if through {
            let frozen = before_land && land_passed;
            let still = row.judged_rev.is_none()
                || frozen
                || row.judged_rev.as_deref() == tree_of(&tools.git, &dir).as_deref();
            if still {
                continue;
            }
        }
        let wt_now = worktree_of(store.conn(), &task.task_id)?;
        let cx = Ctx {
            tools,
            run: &run,
            task: &task,
            worker: &w,
            worktree: wt_now,
            dir: dir.clone(),
            root: root.clone(),
            evidence_dir: evidence_dir.clone(),
            seq,
        };
        store.immediate(|tx| {
            tx.execute(
                "UPDATE task_steps SET status = 'running', rounds = rounds + 1,
                    started_at = COALESCE(started_at, ?4)
                 WHERE task_id = ?1 AND worker_id = ?2 AND seq = ?3",
                params![task.task_id, w.worker_id, seq, util::now()],
            )?;
            Ok(())
        })?;
        let outcome = match run_step(store, &cx, *name) {
            Ok(o) => o,
            Err(e) => Outcome::fail("error", None, &format!("{e:#}")),
        };
        record(store, &task, &w, seq, *name, &outcome)?;
        if let Some(ids) = &outcome.review_task_ids {
            queued_reviews = ids.clone();
        }
        if !outcome.status.is_through() {
            stopped_at = Some(name.as_str().to_string());
            message = format!("{}: {}", about(name.as_str()), outcome.note);
            break;
        }
    }
    let steps = steps_of(store.conn(), &task.task_id, &w.worker_id)?;
    let complete = stopped_at.is_none()
        && steps
            .iter()
            .all(|s| StepStatus::parse(&s.status).is_ok_and(|s| s.is_through()));
    if complete {
        message = format!(
            "{}: every finish step passed. Run: horch ledger done \"<summary>\"",
            task.short()
        );
    }
    Ok(FinishReport {
        task: task.short(),
        steps,
        complete,
        stopped_at,
        message,
        queued_reviews,
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
            .is_some_and(|s| StepStatus::parse(&s.status).is_ok_and(|st| st.is_through()))
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
    let w = workers::get(store.conn(), worker_id)?;
    // A retry after a crash between the write and the report.
    if w.status == "done" {
        let t = w
            .task_id
            .clone()
            .context("this worker finished without a task")?;
        let task = tasks::get(store.conn(), &t)?;
        return Ok(Completed {
            task: task.short(),
            task_id: task.task_id.clone(),
            run_id: task.run_id.clone(),
            review: task.is_review(),
            unlanded: false,
            reviews_in_for: None,
            already: true,
        });
    }
    let (w, task) = held_task(store.conn(), worker_id)?;
    let run = runs::get(store.conn(), &task.run_id)?;
    if summary.trim().is_empty() {
        bail!("give a summary: horch ledger done \"<what you did, files, decisions, gotchas>\"");
    }
    if task.is_review() {
        let findings = task.spec_ref.clone().unwrap_or_default();
        if findings.is_empty() || !Path::new(&findings).is_file() {
            bail!("write your findings file first: {findings}");
        }
    } else if unlanded.is_none() && !all_through(store.conn(), &run, &task.task_id, worker_id)? {
        let steps = steps_of(store.conn(), &task.task_id, worker_id)?;
        let open: Vec<String> = steps
            .iter()
            .filter(|s| !StepStatus::parse(&s.status).is_ok_and(|st| st.is_through()))
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
    let out = store.immediate(|tx| {
        let now = util::now();
        let about = About {
            run: Some(&task.run_id),
            task: Some(&task.task_id),
            worker: Some(worker_id),
        };
        let (status, text, kind) = match unlanded {
            Some(reason) => {
                if let Some(wt) = worktree_of(tx, &task.task_id)? {
                    set_worktree(tx, &wt.worktree_id, WorktreeStatus::Unlanded, Some(reason))?;
                }
                (
                    "cancelled",
                    format!("UNLANDED: {reason} | {summary}"),
                    EventKind::Unlanded,
                )
            }
            None => ("done", summary.to_string(), EventKind::Done),
        };
        tx.execute(
            "UPDATE tasks SET status = ?2, summary = ?3, updated_at = ?4, finished_at = ?4
             WHERE task_id = ?1",
            params![task.task_id, status, text, now],
        )?;
        tx.execute(
            "UPDATE workers SET status = 'done', summary = ?2, end_reason = ?3, ended_at = ?4,
                last_progress_at = ?4
             WHERE worker_id = ?1",
            params![
                worker_id,
                text,
                if unlanded.is_some() {
                    "unlanded"
                } else {
                    "done"
                },
                now
            ],
        )?;
        util::event(
            tx,
            &about,
            Actor::Worker,
            kind,
            &format!("{} {}: {text}", task.short(), status),
        )?;
        // A review task: are all reviews of its parent in?
        let mut reviews_in_for = None;
        if let Some(parent) = task.parent_task_id.clone().filter(|_| task.is_review()) {
            let open: i64 = tx.query_row(
                "SELECT count(*) FROM tasks WHERE parent_task_id = ?1 AND kind = 'review'
                   AND status NOT IN ('done', 'cancelled')",
                [&parent],
                |r| r.get(0),
            )?;
            if open == 0 {
                let p = tasks::get(tx, &parent)?;
                let role = match &p.current_worker_id {
                    Some(id) => workers::get(tx, id)?.role,
                    None => None,
                };
                reviews_in_for = Some((p.short(), role));
            }
        }
        Ok(Completed {
            task: task.short(),
            task_id: task.task_id.clone(),
            run_id: task.run_id.clone(),
            review: task.is_review(),
            unlanded: unlanded.is_some(),
            reviews_in_for,
            already: false,
        })
    })?;
    let _ = w;
    Ok(out)
}
