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
use horch_core::taskledger::finish;
use horch_core::taskledger::model::{EventKind, ProgressSource};
use horch_core::taskledger::{self, tasks, workers};

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
    let worker = workers::get(store.conn(), &wid)?;
    let Some(task_id) = worker.task_id else {
        bail!("refused: worker {wid} has no claimed task. Run horch ledger claim first.");
    };
    let task = tasks::get(store.conn(), &task_id)?;
    if task.is_review() {
        bail!(
            "refused: {} is a review task. A reviewer reports findings and never commits.",
            task.short()
        );
    }
    let Some(wt) = finish::worktree_of(store.conn(), &task_id)? else {
        bail!(
            "refused: {} has no worktree. Run horch ledger worktree new <name> first.",
            task.short()
        );
    };
    let root = std::fs::canonicalize(&wt.path)
        .with_context(|| format!("refused: the worktree {} does not exist", wt.path))?;
    let cwd = std::fs::canonicalize(std::env::current_dir()?)?;
    if !cwd.starts_with(&root) {
        bail!(
            "refused: run horch ledger commit inside the worktree of {}: {}",
            task.short(),
            root.display()
        );
    }
    let paths = args
        .paths
        .iter()
        .map(|p| checked_path(&root, &cwd, p))
        .collect::<Result<Vec<_>>>()?;

    let git = Trusted::new(&ctx.bins.harness.git, &root);
    let mut add: Vec<OsString> = vec!["add".into()];
    if paths.is_empty() {
        add.push("-A".into());
    } else {
        add.push("--".into());
        add.extend(paths.iter().map(|p| p.as_os_str().to_owned()));
    }
    git.run(&add)?;
    let mut commit: Vec<OsString> = vec!["commit".into(), "-m".into(), (&args.message).into()];
    if !paths.is_empty() {
        // Only the named files: paths an earlier attempt staged stay out.
        commit.push("--".into());
        commit.extend(paths.iter().map(|p| p.as_os_str().to_owned()));
    }
    git.run(&commit)?;
    let id = git.run(&["rev-parse".into(), "HEAD".into()])?;
    let short: String = id.chars().take(12).collect();
    store.immediate(|tx| {
        workers::progress(
            tx,
            &wid,
            ProgressSource::Agent,
            EventKind::Progress,
            &format!("commit {short}"),
        )
    })?;
    output::println(&id);
    Ok(ExitCode::SUCCESS)
}

/// `arg` as an absolute path inside `root`, or a refusal. Symlinks in the
/// part that exists are resolved before the check.
fn checked_path(root: &Path, cwd: &Path, arg: &Path) -> Result<PathBuf> {
    let joined = normalize(&cwd.join(arg));
    let resolved = resolve_existing(&joined);
    if !resolved.starts_with(root) {
        bail!(
            "refused: {} is outside the worktree {}",
            arg.display(),
            root.display()
        );
    }
    let inside = resolved.strip_prefix(root).unwrap_or(Path::new(""));
    if inside.components().any(|c| c.as_os_str() == ".git") {
        bail!("refused: {} is inside git metadata", arg.display());
    }
    // A folder below the root that holds its own `.git` is a nested
    // repository: git would read its config, which the worker can write.
    let mut dir = Some(resolved.as_path());
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
    Ok(resolved)
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
    identity: Vec<(String, String)>,
}

impl Trusted {
    fn new(bin: &Path, root: &Path) -> Trusted {
        Trusted {
            bin: bin.to_path_buf(),
            root: root.to_path_buf(),
            identity: global_identity(bin),
        }
    }

    fn run(&self, args: &[OsString]) -> Result<String> {
        let mut cmd = clean_git(&self.bin);
        cmd.arg("-C").arg(&self.root);
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
