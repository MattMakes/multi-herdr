//! `horch ledger commit`: the 1 trusted way a sandboxed worker commits.
//!
//! A Codex worker runs `horch ledger ...` outside its sandbox (the
//! `["horch", "ledger"]` execpolicy rule), and no raw `git` command. So this
//! command must not let the worker steer git into running its own code: it
//! takes the worktree from the ledger, never from the caller, clears the git
//! environment, turns off every config hook that runs a program, and refuses
//! paths that leave the worktree or enter a nested repository, whose
//! `.git/config` the worker can write
//! (docs/live-checks/codex-git.md, T-10 decision 01b).

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{bail, Context, Result};
use clap::Args;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish::{self, WorktreeRow};
use horch_core::taskledger::model::{
    EventKind, ProgressSource, TaskStatus, WorkerStatus, WorktreeStatus,
};
use horch_core::taskledger::{self, tasks, workers, Store};

use super::taskledger::worker_id;
use crate::output;

#[derive(Args)]
pub struct CommitArgs {
    /// The commit message.
    #[arg(short = 'm', long = "message")]
    pub message: String,
    /// The files to stage and commit (after `--`). Default: every change.
    #[arg(last = true)]
    pub paths: Vec<PathBuf>,
}

/// Config pairs on every git call: no hook, no fsmonitor, no signing program.
const SAFE_CONFIG: [&str; 4] = [
    "core.hooksPath=/dev/null",
    "core.fsmonitor=false",
    "commit.gpgsign=false",
    "gpg.program=/usr/bin/false",
];

/// git's empty tree id.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Plain data that names the author and committer; never a program.
const IDENTITY_VARS: [&str; 6] = [
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_DATE",
];

pub fn run(ctx: &RuntimeContext, args: CommitArgs) -> Result<ExitCode> {
    let wid = worker_id(ctx)?;
    let mut store = taskledger::open(ctx)?;
    let (task, wt) = owned_worktree(&store, &wid)?;
    let root = std::fs::canonicalize(&wt.path)
        .with_context(|| format!("refused: the worktree {} does not exist", wt.path))?;
    let cwd = std::fs::canonicalize(std::env::current_dir()?)?;
    if !cwd.starts_with(&root) {
        bail!(
            "refused: run horch ledger commit inside the worktree of {task}: {}",
            root.display()
        );
    }
    let paths = args
        .paths
        .iter()
        .map(|p| checked_path(&root, &cwd, p))
        .collect::<Result<Vec<_>>>()?;

    let bin = &ctx.bins.harness.git;
    check_git_version(bin)?;
    let gitdir = trusted_gitdir(bin, Path::new(&wt.repo_root), Path::new(&wt.path), &root)?;
    let id = commit(&Trusted::new(bin, &root, &gitdir), &args.message, &paths)?;
    // The commit exists now: print its id first, and never fail on the
    // bookkeeping after it, or a retry finds nothing to commit.
    output::println(&id);
    let short: String = id.chars().take(12).collect();
    let recorded = store.immediate(|tx| {
        workers::progress(
            tx,
            &wid,
            ProgressSource::Agent,
            EventKind::Progress,
            &format!("commit {short}"),
        )
    });
    if let Err(e) = recorded {
        eprintln!("horch: committed {short}, but the task ledger did not record it: {e:#}");
    }
    Ok(ExitCode::SUCCESS)
}

/// The short name of the task this worker owns now, and its open worktree,
/// or a refusal. A reaped or requeued worker keeps its old `task_id`, so the
/// task must still name this worker, and a landed worktree takes no commit.
fn owned_worktree(store: &Store, wid: &str) -> Result<(String, WorktreeRow)> {
    let worker = workers::get(store.conn(), wid)?;
    let Some(task_id) = worker.task_id.clone() else {
        bail!("refused: worker {wid} has no claimed task. Run horch ledger claim first.");
    };
    let task = tasks::get(store.conn(), &task_id)?;
    let short = task.short();
    if task.is_review() {
        bail!("refused: {short} is a review task. A reviewer reports findings and never commits.");
    }
    if worker.status()? != WorkerStatus::Working || task.current_worker_id.as_deref() != Some(wid) {
        bail!("refused: worker {wid} does not own {short} now.");
    }
    if !matches!(task.status()?, TaskStatus::Running | TaskStatus::Landing) {
        bail!("refused: {short} is {}, not running.", task.status);
    }
    let Some(wt) = finish::worktree_of(store.conn(), &task_id)? else {
        bail!("refused: {short} has no worktree. Run horch ledger worktree new <name> first.");
    };
    if wt.status != WorktreeStatus::Open.as_str() {
        bail!(
            "refused: the worktree of {short} is {}, not open.",
            wt.status
        );
    }
    Ok((short, wt))
}

/// Stage and commit `paths` (every change when empty); the new commit id.
fn commit(git: &Trusted, message: &str, paths: &[PathBuf]) -> Result<String> {
    let named = paths.iter().map(|p| p.as_os_str().to_owned());
    let mut add: Vec<OsString> = vec!["add".into()];
    let mut commit: Vec<OsString> = vec!["commit".into(), "-m".into(), message.into()];
    if paths.is_empty() {
        add.push("-A".into());
    } else {
        add.push("--".into());
        add.extend(named.clone());
        // Only the named files: paths an earlier attempt staged stay out.
        commit.push("--".into());
        commit.extend(named);
    }
    git.run(&add)?;
    git.run(&commit)?;
    git.run(&["rev-parse".into(), "HEAD".into()])
}

/// The worktree's git dir, from trusted metadata only: the common dir that
/// `git rev-parse` reports in the main checkout the ledger recorded, joined
/// with `worktrees/<name>` from the ledger's worktree path. Refused unless
/// the worktree's `.git` is a file that says exactly `gitdir: <that path>`:
/// the sandbox can rewrite it, and git would read the config it points at.
fn trusted_gitdir(bin: &Path, repo_root: &Path, worktree: &Path, root: &Path) -> Result<PathBuf> {
    let mut cmd = clean_git(bin);
    cmd.arg("-C").arg(repo_root);
    for pair in SAFE_CONFIG {
        cmd.args(["-c", pair]);
    }
    let out = cmd
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .with_context(|| format!("running {}", bin.display()))?;
    if !out.status.success() {
        bail!("refused: {} is not a git checkout", repo_root.display());
    }
    let common = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let Some(name) = worktree.file_name() else {
        bail!(
            "refused: the ledger's worktree path {} has no name",
            worktree.display()
        );
    };
    let gitdir = common.join("worktrees").join(name);
    let dotgit = root.join(".git");
    let text = std::fs::symlink_metadata(&dotgit)
        .ok()
        .filter(|m| m.is_file())
        .and_then(|_| std::fs::read_to_string(&dotgit).ok())
        .unwrap_or_default();
    let line = text.strip_suffix('\n').unwrap_or(&text);
    // git may have written the canonical or the given form of the path.
    let canonical = std::fs::canonicalize(&gitdir).unwrap_or_else(|_| gitdir.clone());
    let exact = [&gitdir, &canonical]
        .iter()
        .any(|p| line == format!("gitdir: {}", p.display()));
    // And the entry must name this worktree back.
    let names_root = std::fs::read_to_string(gitdir.join("gitdir"))
        .ok()
        .and_then(|t| std::fs::canonicalize(t.trim()).ok())
        .is_some_and(|p| p == root.join(".git"));
    if !exact || !gitdir.is_dir() || !names_root {
        bail!(
            "refused: {} was changed; it must say exactly: gitdir: {}",
            dotgit.display(),
            gitdir.display()
        );
    }
    Ok(gitdir)
}

/// Refused unless git is 2.40 or later: older git ignores `GIT_ATTR_SOURCE`,
/// so a `.gitattributes` file could select a filter.
fn check_git_version(bin: &Path) -> Result<()> {
    let out = git_without_env(bin)
        .arg("--version")
        .output()
        .with_context(|| format!("running {}", bin.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let version = text.split_whitespace().nth(2).unwrap_or_default();
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    if (major, minor) < (2, 40) {
        bail!(
            "refused: {} is git {version}. horch ledger commit needs git 2.40 or later \
             (GIT_ATTR_SOURCE), so that no .gitattributes file can select a filter.",
            bin.display()
        );
    }
    Ok(())
}

/// `arg` as an absolute path inside `root`, or a refusal. Symlinks in the
/// part that exists are resolved before the check.
fn checked_path(root: &Path, cwd: &Path, arg: &Path) -> Result<PathBuf> {
    let joined = normalize(&cwd.join(arg));
    let resolved = resolve_existing(&joined);
    // git gets `joined`, so a symlink is committed as the link itself; both
    // it and its target must pass every check.
    for path in [&joined, &resolved] {
        check_inside(root, arg, path)?;
    }
    Ok(joined)
}

fn check_inside(root: &Path, arg: &Path, path: &Path) -> Result<()> {
    if !path.starts_with(root) {
        bail!(
            "refused: {} is outside the worktree {}",
            arg.display(),
            root.display()
        );
    }
    let inside = path.strip_prefix(root).unwrap_or(Path::new(""));
    if inside.components().any(|c| c.as_os_str() == ".git") {
        bail!("refused: {} is inside git metadata", arg.display());
    }
    // A folder below the root that holds its own `.git` is a nested
    // repository: git would read its config, which the worker can write.
    let mut dir = Some(path);
    while let Some(d) = dir {
        if d == root {
            break;
        }
        if d.join(".git").exists() {
            bail!(
                "refused: {} is in the nested repository {}",
                arg.display(),
                d.display()
            );
        }
        dir = d.parent();
    }
    Ok(())
}

/// `.` and `..` removed without touching the disk.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The canonical form of the longest existing prefix of `p`, with the rest
/// appended: a path to a file that is not there yet (or was deleted) still
/// resolves its folders.
fn resolve_existing(p: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut head = p.to_path_buf();
    loop {
        if let Ok(c) = std::fs::canonicalize(&head) {
            return rest.iter().rev().fold(c, |acc, part| acc.join(part));
        }
        match (head.file_name().map(|f| f.to_owned()), head.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                head = parent.to_path_buf();
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// git in the recorded worktree with a cleared environment.
struct Trusted {
    bin: PathBuf,
    root: PathBuf,
    gitdir: PathBuf,
    identity: Vec<(String, String)>,
}

impl Trusted {
    fn new(bin: &Path, root: &Path, gitdir: &Path) -> Trusted {
        Trusted {
            bin: bin.to_path_buf(),
            root: root.to_path_buf(),
            gitdir: gitdir.to_path_buf(),
            identity: global_identity(bin),
        }
    }

    fn run(&self, args: &[OsString]) -> Result<String> {
        let mut cmd = clean_git(&self.bin);
        // The git dir is named, never discovered: discovery would follow the
        // worktree's `.git` file, which the sandbox can rewrite.
        cmd.arg("-C")
            .arg(&self.root)
            .env("GIT_DIR", &self.gitdir)
            .env("GIT_WORK_TREE", &self.root);
        for pair in SAFE_CONFIG {
            cmd.args(["-c", pair]);
        }
        for (key, value) in &self.identity {
            cmd.arg("-c").arg(format!("{key}={value}"));
        }
        for var in IDENTITY_VARS {
            if let Some(v) = std::env::var_os(var) {
                cmd.env(var, v);
            }
        }
        let out = cmd
            .args(args)
            .output()
            .with_context(|| format!("running {}", self.bin.display()))?;
        if !out.status.success() {
            bail!(
                "git {} failed: {}",
                args.first()
                    .map(|a| a.to_string_lossy())
                    .unwrap_or_default(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

/// A git command with every `GIT_*` variable removed, no system config and
/// no global config.
fn clean_git(bin: &Path) -> Command {
    let mut cmd = git_without_env(bin);
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        // Attributes come from the empty tree: no `.gitattributes` file
        // selects a filter.
        .env("GIT_ATTR_SOURCE", EMPTY_TREE)
        .env("GIT_PAGER", "cat")
        .env("GIT_EDITOR", "true")
        .env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

/// `user.name` and `user.email` from the operator's global config, read as
/// values only: with the global config off, a commit keeps its author.
fn global_identity(bin: &Path) -> Vec<(String, String)> {
    ["user.name", "user.email"]
        .into_iter()
        .filter_map(|key| {
            let out = git_without_env(bin)
                .current_dir("/")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .args(["config", "--global", "--get", key])
                .output()
                .ok()?;
            let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (out.status.success() && !value.is_empty()).then(|| (key.to_string(), value))
        })
        .collect()
}

/// A git command with every `GIT_*` variable removed.
fn git_without_env(bin: &Path) -> Command {
    let mut cmd = Command::new(bin);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    cmd.env_remove("ANTHROPIC_API_KEY");
    cmd
}
