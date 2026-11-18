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
    let worktree = || worktree_of(ctx, &attempt.task_id);
    let commits = if args.commit.is_empty() {
        let (worktree, base) = worktree()?;
        commits_since_base(&git, &worktree, &base)?
    } else {
        args.commit
    };
    let files_changed = if args.file.is_empty() {
        files_of(&git, &worktree()?.0, &commits)?
    } else {
        args.file
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
fn worktree_of(ctx: &RuntimeContext, task: &str) -> Result<(PathBuf, String)> {
    let store = horch_core::taskledger::open_existing_read_only(ctx)?
        .context("no task ledger names the worktree; give --commit")?;
    let row = custody::of_task(store.conn(), task)?
        .with_context(|| format!("the task ledger has no worktree of {task}; give --commit"))?;
    Ok((PathBuf::from(row.path), row.base_commit.unwrap_or(row.base)))
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
