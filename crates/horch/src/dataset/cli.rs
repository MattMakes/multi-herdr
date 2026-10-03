//! The command line of `multi-herdr-dataset` (CMP-01).
//!
//! The parse lives in the library, so `tests/dataset_cli.rs` checks every
//! flag without starting the binary.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use horch_core::competition::config::{JudgeMode, RunFlags, Strategy};
use horch_core::ids::TeammateName;
use horch_core::measure::event::OutcomeKind;

#[derive(Debug, Parser)]
#[command(
    name = "multi-herdr-dataset",
    version,
    about = "Competitive dataset runs: N candidates, one judge (expensive)",
    long_about = "Competitive dataset runs for multi-herdr.\n\n\
                  `run` starts N agent candidates on one task, each in its own git\n\
                  worktree, and judges them. Every candidate and the judge are\n\
                  model calls that cost money: set a --budget-usd ceiling.\n\
                  Preflight refuses a run before any worktree or model call.",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Plan a round, run preflight, and start the candidates.
    Run(RunArgs),

    /// Re-enter an experiment where its events stop (after a crash or kill).
    Resume {
        #[arg(value_name = "EXPERIMENT")]
        experiment: String,
    },

    /// Show the experiments and rounds with their state.
    Status {
        /// Only this experiment.
        #[arg(value_name = "EXPERIMENT")]
        experiment: Option<String>,
    },

    /// Write the decided rounds as JSONL under `exports/<label policy>/`.
    Export {
        /// The label policy version to export.
        #[arg(long, value_name = "VERSION")]
        label_policy: Option<String>,
    },

    /// Count the dataset per arm and print the readiness verdict and gaps.
    Readiness {
        /// A `dataset-policy.json` that replaces the built-in thresholds.
        #[arg(long, value_name = "FILE")]
        policy: Option<PathBuf>,
        /// The label policy version to count.
        #[arg(long, value_name = "VERSION")]
        label_policy: Option<String>,
    },

    /// Record what happened to a decided round's change after the merge.
    Outcome {
        #[arg(value_name = "ROUND")]
        round: String,
        #[arg(long, value_enum)]
        kind: OutcomeArg,
        /// The post-merge score, 0 to 1. Defaults to 1 for `verified`, else 0.
        #[arg(long, value_name = "S")]
        score: Option<f64>,
        #[arg(long, value_name = "N")]
        note: Option<String>,
    },

    /// Re-derive the projections of an experiment from its events.
    Rebuild {
        #[arg(value_name = "EXPERIMENT")]
        experiment: String,
    },

    /// The detached judge run of one attempt. The coordinator starts it.
    #[command(hide = true, name = "judge-job")]
    JudgeJob(JudgeJobArgs),
    /// Promote a round's winner onto a branch: from COMPLETE, or from
    /// NEEDS_INTERVENTION when the round has a winner.
    #[command(hide = true)]
    Promote(PromoteArgs),
    /// Move a promoted round's target branch back to where it was.
    #[command(hide = true)]
    Rollback(RollbackArgs),
    /// Remove a round's worktrees. NEEDS_INTERVENTION needs `--force`.
    #[command(hide = true)]
    Cleanup(CleanupArgs),
    /// The dataset workspace's root pane: print the round status every few
    /// seconds until the candidates are done. Reads no input.
    #[command(hide = true)]
    Watch(WatchArgs),
}

#[derive(Debug, clap::Args)]
pub struct WatchArgs {
    #[arg(value_name = "EXPERIMENT")]
    pub experiment: String,
    /// The state root of the dataset (a pane does not inherit the env).
    #[arg(long, value_name = "DIR")]
    pub state_dir: Option<PathBuf>,
    /// The project whose dataset this is.
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,
}

/// `judge-job`: one judge attempt (`evaluation::scheduler::job_args`).
#[derive(Debug, clap::Args)]
pub struct JudgeJobArgs {
    #[arg(long)]
    pub round: String,
    #[arg(long)]
    pub attempt: u32,
    /// The sealed judge-input bundle; the judge runs in it.
    #[arg(long)]
    pub input_dir: PathBuf,
    #[arg(long)]
    pub session_id: String,
    #[arg(long)]
    pub model: String,
    #[arg(long)]
    pub effort: String,
    #[arg(long)]
    pub timeout_s: u64,
}

/// `promote <round> --to <branch>`.
#[derive(Debug, clap::Args)]
pub struct PromoteArgs {
    #[arg(value_name = "ROUND")]
    pub round: String,
    /// The branch the winner goes onto.
    #[arg(long, value_name = "BRANCH")]
    pub to: String,
}

/// `rollback <round>`.
#[derive(Debug, clap::Args)]
pub struct RollbackArgs {
    #[arg(value_name = "ROUND")]
    pub round: String,
}

/// `cleanup <round> [--force] [--prune-branches]`.
#[derive(Debug, clap::Args)]
pub struct CleanupArgs {
    #[arg(value_name = "ROUND")]
    pub round: String,
    /// Clean up a round that needs the operator. Its worktrees go; its
    /// branches stay.
    #[arg(long)]
    pub force: bool,
    /// Also delete each candidate branch after its worktree.
    #[arg(long)]
    pub prune_branches: bool,
}

#[derive(Debug, clap::Args)]
pub struct RunArgs {
    /// The task every candidate works on.
    #[arg(value_name = "TASK")]
    pub task: String,
    /// Candidates in the round.
    #[arg(long, value_name = "N")]
    pub candidates: Option<u32>,
    #[arg(long, value_name = "STRATEGY")]
    pub strategy: Option<Strategy>,
    /// The hard budget ceiling in dollars, exact to 1 µ$, such as 12.50.
    #[arg(long, value_name = "X")]
    pub budget_usd: Option<String>,
    #[arg(long, value_name = "MODE")]
    pub judge: Option<JudgeMode>,
    /// The baseline teammate (slot A).
    #[arg(long, value_name = "TEAMMATE")]
    pub baseline: Option<String>,
    /// Promote the winner onto this branch.
    #[arg(long, value_name = "BRANCH")]
    pub promote_to: Option<String>,
    /// Where the candidate worktrees go. Defaults to the dataset dir.
    #[arg(long, value_name = "DIR")]
    pub worktree_root: Option<PathBuf>,
    /// Run on a dirty work tree (the candidates start from HEAD).
    #[arg(long)]
    pub allow_dirty: bool,
}

impl RunArgs {
    /// The flags as `config::load` takes them.
    pub fn flags(&self) -> Result<RunFlags> {
        Ok(RunFlags {
            task: self.task.clone(),
            candidates: self.candidates,
            strategy: self.strategy,
            budget_usd: self.budget_usd.clone(),
            judge: self.judge,
            baseline: self
                .baseline
                .as_deref()
                .map(TeammateName::new)
                .transpose()?,
            promote_to: self.promote_to.clone(),
            worktree_root: self.worktree_root.clone(),
            allow_dirty: self.allow_dirty,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutcomeArg {
    Regression,
    Revert,
    Verified,
}

impl OutcomeArg {
    pub fn kind(self) -> OutcomeKind {
        match self {
            OutcomeArg::Regression => OutcomeKind::Regression,
            OutcomeArg::Revert => OutcomeKind::Revert,
            OutcomeArg::Verified => OutcomeKind::Verified,
        }
    }

    /// The score when `--score` is not given.
    // SPEC-TODO(Spec B §outcome): the default post-merge score per kind.
    pub fn default_score(self) -> f64 {
        match self {
            OutcomeArg::Verified => 1.0,
            OutcomeArg::Regression | OutcomeArg::Revert => 0.0,
        }
    }
}
