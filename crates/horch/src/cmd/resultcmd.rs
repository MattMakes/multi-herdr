//! `horch result`: the worker writes `outbox/result.json` of its attempt
//! (`docs/specs/fleetd/design.md`, "Workspace layout and file protocol").
//! `--check` is the test of the Stop hooks (FM-6).
//!
//! The attempt is a push attempt: the pane names its `task.yaml` in
//! `HORCH_TASK_YAML`, at `<fleet root>/tasks/<task id>/attempts/<attempt>/`.
//! The file is an immutable message (protocol rules 1 and 2): it is
//! validated by the schema of `domain/result.rs`, then written 1 time, to a
//! temp file first and linked in place. A second `horch result` of 1
//! attempt is refused.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Args, ValueEnum};
use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::task_execution::domain::result::{self as schema, ResultFile};
use horch_core::fleetd::worktree_custody::adapters::sqlite as custody;
use horch_core::fsx;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::git::Git;

/// The pane variable that names the `task.yaml` of a push attempt.
pub const TASK_YAML_ENV: &str = "HORCH_TASK_YAML";

/// The line of `--check` while the attempt has no result and no question.
pub const RESULT_FIRST: &str = "run `horch result` first";

/// The mode of `result.json`: only the user of fleetd and its workers.
const MODE: u32 = 0o600;

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
    let task_yaml = std::env::var_os(TASK_YAML_ENV).filter(|v| !v.is_empty());
    let attempt = match Attempt::resolve(&ctx.paths.fleet_root, task_yaml.as_deref().map(Path::new))
    {
        Ok(attempt) => attempt,
        Err(cause) => return Ok(refuse(&format!("{cause:#}"))),
    };
    if args.check {
        return Ok(check(&attempt));
    }
    match write(ctx, &attempt, args) {
        Ok(path) => {
            println!("wrote {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Err(cause) => Ok(refuse(&format!("{cause:#}"))),
    }
}

/// Print `cause` and exit 1. Exit 1, not 2, so a Stop hook never holds a
/// pane that is not a push attempt.
fn refuse(cause: &str) -> ExitCode {
    eprintln!("horch result: {cause}");
    ExitCode::from(1)
}

/// A push attempt, as its `task.yaml` names it.
#[derive(Debug)]
pub(crate) struct Attempt {
    pub(crate) task_id: String,
    pub(crate) attempt: String,
    /// The attempt folder, with no link in its path.
    dir: PathBuf,
    worktree: PathBuf,
}

impl Attempt {
    /// The attempt of `task_yaml`. It must be
    /// `<fleet_root>/tasks/<task id>/attempts/<attempt>/task.yaml`, and its
    /// `task_id`, `attempt` and `report_to` must be those of its path.
    pub(crate) fn resolve(fleet_root: &Path, task_yaml: Option<&Path>) -> Result<Attempt> {
        let Some(task_yaml) = task_yaml else {
            bail!("this pane is not a push attempt: {TASK_YAML_ENV} is not set");
        };
        let real = task_yaml
            .canonicalize()
            .with_context(|| format!("reading {}", task_yaml.display()))?;
        let root = fleet_root
            .canonicalize()
            .with_context(|| format!("reading the fleet root {}", fleet_root.display()))?;
        let dir = real
            .parent()
            .context("task.yaml has no folder")?
            .to_path_buf();
        let (task_id, attempt) = ids_of(&root, &real).with_context(|| {
            format!(
                "{} is not <fleet root>/tasks/<task id>/attempts/<attempt>/task.yaml of the fleet root {}",
                real.display(),
                root.display()
            )
        })?;
        let text = std::fs::read_to_string(&real)
            .with_context(|| format!("reading {}", real.display()))?;
        let key = |name: &str| {
            yaml_value(&text, name).with_context(|| format!("{} has no key {name}", real.display()))
        };
        for (name, want) in [("task_id", &task_id), ("attempt", &attempt)] {
            let found = key(name)?;
            if &found != want {
                bail!(
                    "{} names {name} {found:?}, but its folder is of {name} {want:?}",
                    real.display()
                );
            }
        }
        let report_to = PathBuf::from(key("report_to")?);
        let outbox_parent = report_to
            .parent()
            .filter(|_| report_to.file_name().is_some_and(|n| n == "outbox"))
            .and_then(|p| p.canonicalize().ok());
        if outbox_parent.as_deref() != Some(dir.as_path()) {
            bail!(
                "{} names report_to {}, which is not the outbox of its folder",
                real.display(),
                report_to.display()
            );
        }
        Ok(Attempt {
            task_id,
            attempt,
            worktree: PathBuf::from(key("worktree")?),
            dir,
        })
    }

    pub(crate) fn outbox(&self) -> PathBuf {
        self.dir.join("outbox")
    }

    fn result_json(&self) -> PathBuf {
        self.outbox().join("result.json")
    }
}

/// The task id and the attempt of `task_yaml`, a real path below `root`.
fn ids_of(root: &Path, task_yaml: &Path) -> Option<(String, String)> {
    let rel = task_yaml.strip_prefix(root).ok()?;
    let parts: Vec<&str> = rel.iter().map(|p| p.to_str()).collect::<Option<_>>()?;
    match parts.as_slice() {
        ["tasks", task, "attempts", attempt, "task.yaml"] => {
            Some((task.to_string(), attempt.to_string()))
        }
        _ => None,
    }
}

/// The value of the top-level key `name` of `task.yaml`. Its writer
/// (`domain/task_files.rs`) puts 1 key on each line, as a plain scalar or a
/// double-quoted scalar whose escapes are those of JSON.
fn yaml_value(text: &str, name: &str) -> Option<String> {
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(": "))?;
    if value.starts_with('"') {
        serde_json::from_str(value).ok()
    } else {
        Some(value.to_string())
    }
}

/// `--check`: exit 0 when the attempt has `result.json` or a question
/// file, else exit 2 with [`RESULT_FIRST`].
fn check(attempt: &Attempt) -> ExitCode {
    let outbox = attempt.outbox();
    if is_regular(&attempt.result_json()) || has_question(&outbox.join("questions")) {
        return ExitCode::SUCCESS;
    }
    eprintln!("{RESULT_FIRST}");
    ExitCode::from(2)
}

/// `path` is a regular file, not a link.
fn is_regular(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}

/// `dir` holds a question file: a regular file that is not hidden and not
/// a temp file (protocol rule 1).
fn has_question(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name();
        let name = name.to_string_lossy();
        !name.starts_with('.') && !name.ends_with(".tmp") && is_regular(&e.path())
    })
}

/// Validate and write `result.json` of `attempt`; its path.
fn write(ctx: &RuntimeContext, attempt: &Attempt, args: ResultArgs) -> Result<PathBuf> {
    let path = attempt.result_json();
    if std::fs::symlink_metadata(&path).is_ok() {
        bail!(
            "{} exists; result.json of an attempt is written 1 time",
            path.display()
        );
    }
    let (Some(status), Some(summary)) = (args.status, args.summary) else {
        bail!("give --status and --summary");
    };
    let git = Git::new(&ctx.bins.harness.git);
    let commits = if args.commit.is_empty() {
        commits_since_base(ctx, &git, &attempt.worktree)?
    } else {
        args.commit
    };
    let files_changed = if args.file.is_empty() {
        files_of(&git, &attempt.worktree, &commits)?
    } else {
        args.file
    };
    let file = ResultFile {
        task: attempt.task_id.clone(),
        attempt: attempt.attempt.clone(),
        status: status.into(),
        summary,
        commits,
        files_changed,
        acceptance_self_check: Vec::new(),
        notes_for_next: args.notes,
    };
    let bytes = file.to_json().into_bytes();
    let max_bytes = config_file::load(&ctx.paths.fleet_root)?.result_max_bytes;
    schema::validate(&bytes, &attempt.task_id, &attempt.attempt, max_bytes)?;
    let outbox = attempt.outbox();
    std::fs::create_dir_all(&outbox).with_context(|| format!("creating {}", outbox.display()))?;
    match fsx::create_immutable(&path, &bytes, MODE) {
        Ok(fsx::Created::Written) => Ok(path),
        Ok(fsx::Created::AlreadyIdentical) | Err(fsx::FsxError::Conflict { .. }) => bail!(
            "{} exists; result.json of an attempt is written 1 time",
            path.display()
        ),
        Err(e) => Err(e.into()),
    }
}

/// The commits of the branch of `worktree` since its base, oldest first.
/// The base is the one that the ledger recorded for the tree.
fn commits_since_base(ctx: &RuntimeContext, git: &Git, worktree: &Path) -> Result<Vec<String>> {
    let store = horch_core::taskledger::open_existing_read_only(ctx)?
        .context("no task ledger names the worktree; give --commit")?;
    let row = custody::by_path(store.conn(), &worktree.to_string_lossy())?.with_context(|| {
        format!(
            "the task ledger has no worktree {}; give --commit",
            worktree.display()
        )
    })?;
    let base = row.base_commit.unwrap_or(row.base);
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
    fn yaml_value_reads_a_plain_and_a_quoted_scalar() {
        let text = "task: T1\nworktree: \"/a b/\\\"c\\\"\"\nattempt: A1\n";
        assert_eq!(yaml_value(text, "task").as_deref(), Some("T1"));
        assert_eq!(yaml_value(text, "worktree").as_deref(), Some("/a b/\"c\""));
        assert_eq!(yaml_value(text, "attempt").as_deref(), Some("A1"));
        assert_eq!(yaml_value(text, "task_id"), None);
    }

    #[test]
    fn ids_of_needs_the_layout_of_an_attempt() {
        let root = Path::new("/f");
        assert_eq!(
            ids_of(root, Path::new("/f/tasks/T1/attempts/A2/task.yaml")),
            Some(("T1".into(), "A2".into()))
        );
        for bad in [
            "/g/tasks/T1/attempts/A2/task.yaml",
            "/f/tasks/T1/A2/task.yaml",
            "/f/tasks/T1/attempts/A2/other.yaml",
            "/f/x/tasks/T1/attempts/A2/task.yaml",
        ] {
            assert_eq!(ids_of(root, Path::new(bad)), None, "{bad}");
        }
    }
}
