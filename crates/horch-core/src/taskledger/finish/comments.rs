//! The comment-sweep step (CMS-01..CMS-06, docs/specs/comment-sweep.md):
//! the comments a task added go before it lands. A worker never starts
//! subagents, so the sweep is a review task that a `comment-remover` worker
//! claims. Unlike a code-sweep reviewer, it edits: it deletes comments in
//! the author's work folder and commits there. The author waits, as it
//! waits for reviewers. The commit moves the tree, so `check` runs again
//! before the land.
//!
//! The sweep state belongs to the task, not the worker: a replacement
//! reuses it. A later commit of the author is swept too, from the last
//! swept tree, at most [`MAX_SWEEPS`] times in a task. A sweep that the
//! orchestrator cancels passes the step: the cancel is its ruling.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{codesweep, Ctx, Outcome};
use crate::taskledger::model::{Actor, TaskKind, TaskStatus, WorktreeMode};
use crate::taskledger::store::Store;
use crate::taskledger::tasks::{self, NewTask, Task};
use crate::taskledger::workers;

/// The teammate that claims a sweep.
pub const PERSONA: &str = "comment-remover";

/// The most sweeps of 1 task: the first, and 1 of what the author
/// committed after it.
pub const MAX_SWEEPS: u32 = 2;

const EXCLUDE_DELETED_FILES: &str = "--diff-filter=d";

/// The priority of a sweep: the author waits for it, so it goes before
/// ordinary work, like a code-sweep review.
const AHEAD_OF_WORK: i64 = -500;

/// The wait of the author, the same kind as the wait for reviewers: the
/// watchdog treats both alike.
const WAIT: &str = "reviewers";

/// File endings with no comment to remove: prose, data and images.
const NOT_CODE: &[&str] = &[
    ".md", ".txt", ".json", ".lock", ".csv", ".snap", ".svg", ".png", ".jpg", ".jpeg", ".gif",
    ".webp", ".ico", ".pdf",
];

/// The sweeps of 1 task so far.
#[derive(Debug, Serialize, Deserialize)]
struct State {
    /// The number of the last sweep queued.
    sweep: u32,
    /// Its task.
    task_id: String,
    /// The report its worker writes.
    report: String,
    /// The tree when `finish` first saw the sweep ended: the next sweep
    /// starts there. `None` while the sweep is open.
    swept: Option<String>,
}

fn state_path(cx: &Ctx<'_>) -> PathBuf {
    super::task_dir(&cx.tools.evidence_root, &cx.task.task_id).join("comment-sweep.json")
}

fn load(cx: &Ctx<'_>) -> Option<State> {
    serde_json::from_str(&std::fs::read_to_string(state_path(cx)).ok()?).ok()
}

fn save(cx: &Ctx<'_>, state: &State) -> Result<()> {
    let path = state_path(cx);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(state)?)?;
    Ok(())
}

/// Whether `file` can hold a comment worth a sweep.
pub fn is_code(file: &str) -> bool {
    let lower = file.to_lowercase();
    !NOT_CODE.iter().any(|end| lower.ends_with(end))
}

/// The ticket of a sweep: the facts of this task. How to sweep is the
/// persona's own text.
fn ticket_text(cx: &Ctx<'_>, diff: &Path, files: &[String], report: &Path) -> String {
    format!(
        "# Comment sweep {short}\n\n\
         - The work folder, with the author's branch checked out: {dir}\n\
         - The diff to sweep: {diff}\n\
         - The changed code files: {files}\n\
         - Your report: {report}\n\n\
         Sweep only the lines that the diff adds or changes. Commit in the work folder, \
         with the exact files you changed. Write your report, then run: \
         horch ledger done \"<1 line: comments removed, comments kept>\"\n",
        short = cx.task.short(),
        dir = cx.dir.display(),
        diff = diff.display(),
        files = files.join(", "),
        report = report.display(),
    )
}

fn sweep_task(cx: &Ctx<'_>, ticket: &Path, report: &Path) -> NewTask {
    let mut t = NewTask::work(
        &format!("Comment sweep {}", cx.task.short()),
        &ticket.to_string_lossy(),
        PERSONA,
    );
    t.kind = TaskKind::Review;
    t.parent_task_id = Some(cx.task.task_id.clone());
    t.worktree_mode = WorktreeMode::Shared;
    t.priority = AHEAD_OF_WORK;
    t.spec_ref = Some(report.to_string_lossy().into_owned());
    t
}

fn wait(store: &Store, cx: &Ctx<'_>, task_id: &str) -> Result<()> {
    workers::set_wait(
        store.conn(),
        &cx.worker.worker_id,
        WAIT,
        "waiting for the comment-sweep task",
        &[task_id.to_string()],
        7 * 24 * 3600,
    )
}

/// Queue sweep `number` of the change since `since` (a tree), else since
/// the task's diff base. `not_applicable` when that change has no code file.
fn queue(
    store: &mut Store,
    cx: &Ctx<'_>,
    tree: &str,
    number: u32,
    since: Option<&str>,
) -> Result<Outcome> {
    let git = &cx.tools.git;
    let base = cx.diff_base()?;
    let range = codesweep::diff_range(&base, since);
    let mut args = vec!["diff"];
    args.extend(range.iter().map(String::as_str));
    let diff = git.run(&cx.dir, &args)?;
    args.splice(1..1, ["--name-only", EXCLUDE_DELETED_FILES]);
    let names = git.run(&cx.dir, &args)?;
    let files: Vec<String> = names
        .lines()
        .filter(|f| is_code(f))
        .map(String::from)
        .collect();
    if files.is_empty() {
        return Ok(Outcome::not_applicable("the change has no code file").judged(Some(tree.into())));
    }
    let dir = codesweep::review_root(&cx.root)?
        .join(format!("T{}-{}", cx.task.seq, &cx.task.task_id[..8]))
        .join(format!("comment-sweep-{number}"));
    std::fs::create_dir_all(&dir)?;
    let diff_path = dir.join("diff.patch");
    std::fs::write(&diff_path, diff)?;
    let report = dir.join("report.md");
    discard_earlier_attempt(&report);
    let ticket = dir.join("ticket.md");
    std::fs::write(&ticket, ticket_text(cx, &diff_path, &files, &report))?;
    let new = sweep_task(cx, &ticket, &report);
    let added = store.immediate(|tx| tasks::add(tx, cx.run, &new, Actor::Worker))?;
    save(
        cx,
        &State {
            sweep: number,
            task_id: added.task_id.clone(),
            report: report.to_string_lossy().into_owned(),
            swept: None,
        },
    )?;
    wait(store, cx, &added.task_id)?;
    let note = format!(
        "sweep {number}: task {} queued for {PERSONA}. It commits in your work folder, so do not edit there. \
         horch wakes the orchestrator to spawn it, and tells you when it is done. End your turn now",
        added.short()
    );
    Ok(Outcome::waiting(&note, vec![added.task_id]).judged(Some(tree.into())))
}

fn discard_earlier_attempt(report: &Path) {
    let _ = std::fs::remove_file(report);
}

fn ended(task: &Task) -> bool {
    task.status == TaskStatus::Done.as_str() || task.status == TaskStatus::Cancelled.as_str()
}

/// The outcome of the sweep that `state` names, once its task ended.
fn passed(state: &State, sweep: &Task, tree: &str) -> Outcome {
    let report = Some(PathBuf::from(&state.report)).filter(|p| p.is_file());
    let out = if sweep.status == TaskStatus::Cancelled.as_str() {
        Outcome::pass(
            "skipped",
            report,
            &format!(
                "sweep {}: the orchestrator cancelled {}",
                state.sweep,
                sweep.short()
            ),
        )
    } else {
        Outcome::pass(
            "swept",
            report,
            &format!("sweep {}: {} is done", state.sweep, sweep.short()),
        )
    };
    out.judged(Some(tree.into()))
}

pub fn step(store: &mut Store, cx: &Ctx<'_>) -> Result<Outcome> {
    let tree = cx.tools.git.tree(&cx.dir)?;
    let Some(mut state) = load(cx) else {
        return queue(store, cx, &tree, 1, None);
    };
    let sweep = tasks::get(store.conn(), &state.task_id)?;
    if !ended(&sweep) {
        if !workers::waiting(store.conn(), &cx.worker.worker_id)?
            .iter()
            .any(|w| w.kind == WAIT)
        {
            wait(store, cx, &state.task_id)?;
        }
        let note = format!(
            "sweep {}: waiting for {}. End your turn; horch tells you when it is done",
            state.sweep,
            sweep.short()
        );
        return Ok(Outcome::waiting(&note, vec![state.task_id]).judged(Some(tree)));
    }
    match state.swept.clone() {
        None => {
            workers::clear_wait(store.conn(), &cx.worker.worker_id, WAIT)?;
            state.swept = Some(tree.clone());
            save(cx, &state)?;
            Ok(passed(&state, &sweep, &tree))
        }
        Some(swept) if swept == tree => Ok(passed(&state, &sweep, &tree)),
        Some(_) if state.sweep >= MAX_SWEEPS => Ok(Outcome::pass(
            "swept",
            None,
            &format!(
                "the {MAX_SWEEPS} sweeps of this task are spent; the change since the last one is not swept"
            ),
        )
        .judged(Some(tree))),
        Some(swept) => queue(store, cx, &tree, state.sweep + 1, Some(&swept)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_data_and_images_are_not_code() {
        for f in [
            "README.md",
            "docs/a.TXT",
            "x.json",
            "Cargo.lock",
            "logo.png",
        ] {
            assert!(!is_code(f), "{f}");
        }
        for f in [
            "src/lib.rs",
            "a.ts",
            "Makefile",
            "justfile",
            "x.py",
            "conf.toml",
        ] {
            assert!(is_code(f), "{f}");
        }
    }
}
