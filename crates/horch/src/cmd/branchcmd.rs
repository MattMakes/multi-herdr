use std::path::Path;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::fleetd::integration::application::commands::{self, GitRefTips, RefTips, Rescued};
use horch_core::fleetd::integration::application::landing::GitBranchRescues;
use horch_core::fleetd::worktree_custody::adapters::git::{repo_git_dir, CliWorktreeGit};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::tasks::Task;
use horch_core::taskledger::{self, Store};
use serde::Deserialize;
use serde_json::{json, Value};

use super::operator_request::{self, Answer};
use crate::output;

#[derive(Subcommand)]
pub enum BranchCommand {
    /// Abandon a task branch. fleetd writes a rescue ref of the branch
    /// first.
    Abandon {
        /// The task branch.
        branch: String,
    },
}

pub fn run(ctx: &RuntimeContext, command: BranchCommand) -> Result<ExitCode> {
    match command {
        BranchCommand::Abandon { branch } => {
            let store = read_only(ctx)?;
            let payload = rescued(ctx, &store, &branch)?;
            drop(store);
            operator_request::run(ctx, Kind::BranchAbandon, payload, |r| {
                output::println(&line(&serde_json::from_value(r.clone())?));
                Ok(())
            })
        }
    }
}

fn rescued(ctx: &RuntimeContext, store: &Store, branch: &str) -> Result<Value> {
    let repo = commands::repo_of(store.conn(), branch)?;
    let git = rescue_git(ctx, &repo, branch)?;
    let rescues = GitBranchRescues { git: &git };
    let r = commands::rescue(store.conn(), &rescues, &repo, branch)?;
    rescue_payload(ctx, &repo, r)
}

pub(super) fn rescue_git(ctx: &RuntimeContext, repo: &Path, what: &str) -> Result<CliWorktreeGit> {
    let git = CliWorktreeGit::new(&ctx.bins.harness.git, &ctx.paths.fleet_root)
        .reading(&[repo.to_path_buf()])
        .writing(&[repo_git_dir(repo)?]);
    if let Some(gap) = git.sandbox_gap() {
        bail!("{gap}; horch writes a rescue ref only in the sandbox, so {what} is not abandoned");
    }
    Ok(git)
}

pub(super) fn rescue_payload(ctx: &RuntimeContext, repo: &Path, r: Rescued) -> Result<Value> {
    let tip = match &r.rescue_ref {
        Some(name) => {
            let git = CliWorktreeGit::new(&ctx.bins.harness.git, &ctx.paths.fleet_root)
                .reading(&[repo.to_path_buf()]);
            let refs = GitRefTips { git };
            Some(
                refs.tip(repo, name)?
                    .with_context(|| format!("the rescue ref {name} does not exist"))?,
            )
        }
        None => None,
    };
    Ok(json!({ "branch": r.branch, "rescue_ref": r.rescue_ref, "tip": tip }))
}

fn read_only(ctx: &RuntimeContext) -> Result<Store> {
    taskledger::open_existing_read_only(ctx)?.with_context(|| {
        format!(
            "the task ledger {} does not exist. fleetd creates it at its start: run horch fleetd check",
            ctx.paths.ledger_db.display()
        )
    })
}

#[derive(Deserialize)]
struct Abandoned {
    branch: String,
    rescue_ref: Option<String>,
    already: bool,
}

fn line(done: &Abandoned) -> String {
    let kept = match &done.rescue_ref {
        Some(name) => format!("its rescue ref {name} keeps its tip"),
        None => "git has no such branch, so it has no tip to keep".to_string(),
    };
    if done.already {
        format!("{} was abandoned already; {kept}.", done.branch)
    } else {
        format!("{} is abandoned; {kept}.", done.branch)
    }
}

pub(super) fn abandon_branches_of(ctx: &RuntimeContext, store: &Store, task: &Task) {
    let names = match commands::open_branches_of_task(store.conn(), &task.task_id) {
        Ok(names) => names,
        Err(e) => {
            output::println(&format!(
                "The task branches of {} are not read: {e:#}. The next pass of fleetd abandons them.",
                task.short()
            ));
            return;
        }
    };
    for name in names {
        let sent = rescued(ctx, store, &name)
            .and_then(|p| operator_request::request(ctx, Kind::BranchAbandon, p));
        match sent {
            Ok(Answer::Applied(r)) => match serde_json::from_value(r) {
                Ok(done) => output::println(&line(&done)),
                Err(e) => output::println(&format!("{name}: the result of fleetd is not read: {e}")),
            },
            Ok(Answer::Queued) => {}
            Ok(Answer::Refused(reason)) => output::println(&format!(
                "{name} is not abandoned yet: refused: {reason}. The next pass of fleetd writes its rescue ref and abandons it."
            )),
            Err(e) => output::println(&format!(
                "{name} is not abandoned yet: {e:#}. The next pass of fleetd writes its rescue ref and abandons it."
            )),
        }
    }
}
