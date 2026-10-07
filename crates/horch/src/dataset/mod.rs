//! `multi-herdr-dataset`: the separate, expensive entrypoint (OD2, CMP-01).
//!
//! The binary in `src/bin/multi-herdr-dataset.rs` parses [`cli::Cli`],
//! builds the [`RuntimeContext`] and calls [`dispatch`]. Each command lives
//! in its own module here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use horch_core::measure::paths::DatasetPaths;
use horch_core::runtime::RuntimeContext;
use horch_core::vcs::git::{GitCli, GitClient};

pub(crate) mod cleanup;
pub mod cli;
pub(crate) mod export;
pub(crate) mod judge_job;
pub mod outcome;
pub(crate) mod preflight;
pub(crate) mod promote;
pub(crate) mod readiness;
pub(crate) mod rebuild;
pub(crate) mod rollback;
pub(crate) mod run;
pub mod status;
pub(crate) mod watch;

use cli::{Cli, Command};

/// The exit codes of `multi-herdr-dataset` (B2).
pub mod exit {
    pub const SUCCESS: u8 = 0;
    /// Any error that is not one of the codes below.
    pub const FAILURE: u8 = 1;
    /// A usage error: `run --detach` without a pane to report to.
    pub(crate) const USAGE: u8 = 2;
    /// The budget or the quota refused the run.
    pub(crate) const BUDGET_REFUSED: u8 = 3;
    /// Preflight failed: no worktree was created and no model was called.
    pub(crate) const PREFLIGHT_FAILED: u8 = 4;
    /// The round needs the operator.
    pub const NEEDS_INTERVENTION: u8 = 5;
    /// The round was rejected.
    pub const REJECTED: u8 = 6;
}

/// Run the parsed command line. `env` holds the process variables that the
/// environment snapshot may keep (SEC-02); the binary collects them once.
///
/// Every command except `judge-job` first resolves its project (see
/// [`resolve_project`]). The operator commands print the target repo first.
pub fn dispatch(ctx: &mut RuntimeContext, env: &BTreeMap<String, String>, cli: Cli) -> Result<u8> {
    let command = cli.command;
    // The judge job runs in its sealed bundle; the coordinator passes its
    // project in `HORCH_PROJECT_DIR`.
    if !matches!(command, Command::JudgeJob(_)) {
        let git = GitCli::new(ctx.bins.harness.git.clone());
        let ambient = ctx.paths.project_dir.clone();
        ctx.paths.project_dir = resolve_project(
            cli.project.as_deref(),
            ctx.paths.cwd.as_deref(),
            ambient.as_deref(),
            |dir| {
                let top = git.toplevel(dir).ok()?;
                // `HORCH_PROJECT_DIR` first: the cwd is already resolved.
                Some(match ambient.as_deref() {
                    Some(a) if spelled_as(&top, a) != top => spelled_as(&top, a),
                    _ => spelled_as(&top, dir),
                })
            },
        );
        // `run --detach` prints 1 line on a pass (FDS-10).
        let detached = matches!(&command, Command::Run(args) if args.detach);
        if !detached
            && matches!(
                command,
                Command::Run(_)
                    | Command::Resume { .. }
                    | Command::Status { .. }
                    | Command::Promote(_)
                    | Command::Rollback(_)
                    | Command::Cleanup(_)
            )
        {
            if let Some(project) = &ctx.paths.project_dir {
                let head = git.head(project).ok();
                println!("{}", target_line(project, head.as_deref()));
            }
        }
    }
    match command {
        Command::Run(args) => run::run(ctx, env, &args),
        Command::Resume {
            experiment,
            report_to,
        } => run::resume_reporting(ctx, env, &experiment, report_to.as_deref()),
        Command::Status { experiment } => status::status(ctx, experiment.as_deref()),
        Command::Export { label_policy } => export::export(ctx, label_policy.as_deref()),
        Command::Readiness {
            policy,
            label_policy,
        } => readiness::readiness(ctx, policy.as_deref(), label_policy.as_deref()),
        Command::Outcome {
            round,
            kind,
            score,
            note,
        } => outcome::outcome(
            ctx,
            &round,
            kind.kind(),
            score.unwrap_or(kind.default_score()),
            note,
        ),
        Command::Rebuild { experiment } => rebuild::rebuild(ctx, &experiment),
        Command::JudgeJob(args) => judge_job::judge_job(ctx, &args),
        Command::Promote(args) => promote::promote(ctx, env, &args),
        Command::Rollback(args) => rollback::rollback(ctx, &args),
        Command::Cleanup(args) => cleanup::cleanup(ctx, &args),
        Command::Watch(args) => watch::watch(ctx, &args),
    }
}

/// The project a dataset command works on (dataset design §2.1), first match:
///
/// 1. `--project <dir>` (relative to the cwd);
/// 2. the git top level of the cwd;
/// 3. `ambient`: `$HORCH_PROJECT_DIR`, else the cwd.
///
/// A fleet pane sets `HORCH_PROJECT_DIR` to the fleet's repo, so the cwd's
/// repo must win over it.
pub fn resolve_project(
    flag: Option<&Path>,
    cwd: Option<&Path>,
    ambient: Option<&Path>,
    toplevel: impl Fn(&Path) -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(dir) = flag {
        return Some(match cwd {
            Some(cwd) if dir.is_relative() => cwd.join(dir),
            _ => dir.to_path_buf(),
        });
    }
    cwd.and_then(toplevel)
        .or_else(|| ambient.map(Path::to_path_buf))
}

/// `top` (as git prints it: symlinks resolved) in the spelling of `dir`,
/// when `dir` is `top` or under it; else `top`. The ledger and the dataset
/// dir are keyed on the path text, so `/var/x` must not become
/// `/private/var/x`.
pub fn spelled_as(top: &Path, dir: &Path) -> PathBuf {
    let spelled = std::fs::canonicalize(dir).ok().and_then(|real| {
        let depth = real.strip_prefix(top).ok()?.components().count();
        let candidate = dir.ancestors().nth(depth)?;
        (std::fs::canonicalize(candidate).ok()? == top).then(|| candidate.to_path_buf())
    });
    spelled.unwrap_or_else(|| top.to_path_buf())
}

/// `target: <repo> @ <HEAD>`: the first line of an operator command.
pub fn target_line(project: &Path, head: Option<&str>) -> String {
    let head = head.map_or("no HEAD", |h| &h[..h.len().min(12)]);
    format!("target: {} @ {head}", project.display())
}

/// The dataset of the context's project (OD3).
pub(crate) fn dataset_paths(ctx: &RuntimeContext) -> Result<DatasetPaths> {
    Ok(DatasetPaths::new(
        &ctx.paths.state_root,
        &ctx.paths.project()?,
    ))
}
