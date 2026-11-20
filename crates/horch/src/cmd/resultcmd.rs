//! `horch result`: the worker writes `outbox/result.json` of its attempt
//! (`docs/specs/fleetd/design.md`, "Workspace layout and file protocol").
//! `--check` is the test of the Stop hooks (FM-6).
//!
//! The attempt is the own attempt of this process: the live attempt whose
//! wrapper is in its parent chain (`docs/tickets/fleetd-p5/MAP.md`, Q3).
//! The outbox writer finds it and writes the file: an immutable message,
//! validated by the schema of `domain/result.rs`, then written 1 time
//! (protocol rules 1 and 2). A second `horch result` of 1 attempt is
//! refused.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Args, ValueEnum};
use horch_core::fleetd::task_execution::adapters::outbox_writer::OutboxWriter;
use horch_core::fleetd::task_execution::domain::outbox::{QUESTIONS, RESULT_JSON};
use horch_core::fleetd::task_execution::domain::result::{self as schema, ResultFile};
use horch_core::fleetd::worktree_custody::adapters::sqlite as custody;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::tasks;

/// The line of `--check` while the attempt has no result and no question.
pub const RESULT_FIRST: &str = "run `horch result` first";

/// The closed list of statuses of `result.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ResultStatus {
    Completed,
    Blocked,
    Failed,
}

impl From<ResultStatus> for schema::ResultStatus {
    fn from(s: ResultStatus) -> Self {
        match s {
            ResultStatus::Completed => schema::ResultStatus::Completed,
            ResultStatus::Blocked => schema::ResultStatus::Blocked,
            ResultStatus::Failed => schema::ResultStatus::Failed,
        }
    }
}

#[derive(Args)]
pub struct ResultArgs {
    /// What the attempt did, in 1 paragraph.
    #[arg(long)]
    pub summary: Option<String>,
    /// The claim of the worker. Only the verifier moves a task to done.
    #[arg(long, value_enum)]
    pub status: Option<ResultStatus>,
    /// A commit of the attempt. Give it 1 time for each commit. The
    /// default is each commit of the branch since its base.
    #[arg(long, value_name = "SHA")]
    pub commit: Vec<String>,
    /// A file that the attempt changed. Give it 1 time for each file. The
    /// default is each file of the commits.
    #[arg(long, value_name = "PATH")]
    pub file: Vec<String>,
    /// Notes for the next attempt.
    #[arg(long)]
    pub notes: Option<String>,
    /// Write nothing. Exit 2 while both `result.json` and a question file
    /// are missing: the test of the Stop hooks.
    #[arg(long)]
    pub check: bool,
}

/// `horch result`.
pub fn run(ctx: &RuntimeContext, args: ResultArgs) -> Result<ExitCode> {
    let out = match super::askcmd::own_writer(ctx) {
        Ok(Ok(out)) => out,
        Ok(Err(no)) => return Ok(refuse(&no.to_string())),
        Err(cause) => return Ok(refuse(&format!("{cause:#}"))),
    };
    if args.check {
        return Ok(check(out.outbox()));
    }
    match write(ctx, &out, args) {
        Ok(path) => {
            println!("wrote {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Err(cause) => Ok(refuse(&format!("{cause:#}"))),
    }
}

/// Print `cause` and exit 1. Exit 1, not 2, so a Stop hook never holds a
/// pane that is not in a live attempt.
fn refuse(cause: &str) -> ExitCode {
    eprintln!("horch result: {cause}");
    ExitCode::from(1)
}

/// `--check`: exit 0 when the outbox has `result.json` or a question
/// file, else exit 2 with [`RESULT_FIRST`].
fn check(outbox: &Path) -> ExitCode {
    if is_regular(&outbox.join(RESULT_JSON)) || has_question(&outbox.join(QUESTIONS)) {
        return ExitCode::SUCCESS;
    }
    eprintln!("{RESULT_FIRST}");
    ExitCode::from(2)
}

/// `path` is a regular file, not a link.
pub(crate) fn is_regular(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}

/// `dir` holds a question file: a regular file that is not hidden and not
/// a temp file (protocol rule 1).
pub(crate) fn has_question(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name();
        let name = name.to_string_lossy();
        !name.starts_with('.') && !name.ends_with(".tmp") && is_regular(&e.path())
    })
}

/// Build `result.json` of the own attempt from `args` and write it
/// through `out`; its path.
fn write(ctx: &RuntimeContext, out: &OutboxWriter, args: ResultArgs) -> Result<PathBuf> {
    let path = out.outbox().join(RESULT_JSON);
    if std::fs::symlink_metadata(&path).is_ok() {
        bail!(
            "{} exists; result.json of an attempt is written 1 time",
            path.display()
        );
    }
    let (Some(status), Some(summary)) = (args.status, args.summary) else {
        bail!("give --status and --summary");
    };
    let attempt = out.attempt();
    let git = Git::new(&ctx.bins.harness.git);
    let need = args.commit.is_empty() || args.file.is_empty();
    let wt = if need {
        worktree_of(ctx, &attempt.task_id)?
    } else {
        None
    };
    let commits = match (&wt, args.commit.is_empty()) {
        (_, false) => args.commit,
        (Some((worktree, base)), true) => commits_since_base(&git, worktree, base)?,
        (None, true) => Vec::new(),
    };
    let files_changed = match (&wt, args.file.is_empty()) {
        (_, false) => args.file,
        (Some((worktree, _)), true) => files_of(&git, worktree, &commits)?,
        (None, true) => Vec::new(),
    };
    out.write_result(&ResultFile {
        task: attempt.task_id.clone(),
        attempt: attempt.attempt().to_string(),
        status: status.into(),
        summary,
        commits,
        files_changed,
        acceptance_self_check: Vec::new(),
        notes_for_next: args.notes,
        handoff: None,
    })
}

/// The worktree of `task` and the base that the ledger recorded for it.
fn worktree_of(ctx: &RuntimeContext, task: &str) -> Result<Option<(PathBuf, String)>> {
    let store = horch_core::taskledger::open_existing_read_only(ctx)?
        .context("no task ledger names the worktree; give --commit")?;
    let review = tasks::get(store.conn(), task)?.is_review();
    let row = custody::of_task(store.conn(), task)?;
    let wt = row.map(|r| (PathBuf::from(r.path), r.base_commit.unwrap_or(r.base)));
    own_worktree(task, review, wt)
}

fn own_worktree(
    task: &str,
    review: bool,
    wt: Option<(PathBuf, String)>,
) -> Result<Option<(PathBuf, String)>> {
    match wt {
        Some(wt) => Ok(Some(wt)),
        None if review => Ok(None),
        None => bail!("the task ledger has no worktree of {task}; give --commit"),
    }
}

/// The commits of the branch of `worktree` since `base`, oldest first.
fn commits_since_base(git: &Git, worktree: &Path, base: &str) -> Result<Vec<String>> {
    let range = format!("{base}..HEAD");
    let out = git.run(worktree, &["rev-list", "--reverse", &range])?;
    Ok(out.lines().map(str::to_string).collect())
}

/// The files that `commits` changed, sorted, each 1 time.
fn files_of(git: &Git, worktree: &Path, commits: &[String]) -> Result<Vec<String>> {
    let mut files = std::collections::BTreeSet::new();
    for commit in commits {
        let out = git.run(
            worktree,
            &[
                "show",
                "--no-renames",
                "--pretty=format:",
                "--name-only",
                commit,
            ],
        )?;
        files.extend(out.lines().filter(|l| !l.is_empty()).map(str::to_string));
    }
    Ok(files.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_review_task_with_no_worktree_has_no_commits_to_read() {
        assert_eq!(own_worktree("T-1", true, None).unwrap(), None);
    }

    #[test]
    fn a_work_task_with_no_worktree_needs_its_commits() {
        let e = own_worktree("T-1", false, None).unwrap_err();
        assert_eq!(
            e.to_string(),
            "the task ledger has no worktree of T-1; give --commit"
        );
    }

    #[test]
    fn a_task_with_a_worktree_reads_it() {
        let wt = Some((PathBuf::from("/w"), "base".to_string()));
        assert_eq!(own_worktree("T-1", true, wt.clone()).unwrap(), wt);
        assert_eq!(own_worktree("T-1", false, wt.clone()).unwrap(), wt);
    }
}
