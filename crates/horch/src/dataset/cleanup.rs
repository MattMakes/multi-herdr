//! `multi-herdr-dataset cleanup <round> [--force] [--prune-branches]`: the
//! operator removes a round's worktrees (dataset design §5.1, PRO-07).
//!
//! The coordinator cleans up a round at its end. This command is for the
//! rest: a round in NEEDS_INTERVENTION (only with `--force`, exit 5
//! without), or a cleanup that a crash stopped (CLEANUP). Branches stay
//! unless `--prune-branches`. A worktree that cannot be removed is
//! recorded as `worktree.cleanup_failed`. The cleanup itself removes the
//! round's empty directories
//! ([`horch_core::competition::cleanup::remove_empty_dirs`]).

use anyhow::Result;
use horch_core::competition::cleanup::{CleanupOptions, CleanupOutcome, RoundCleanup};
use horch_core::competition::model::RoundState;
use horch_core::competition::promotion::FaultFired;
use horch_core::measure::recorder::JsonlRecorder;
use horch_core::measure::store::StoreOptions;
use horch_core::runtime::fault::ABORT_EXIT_CODE;
use horch_core::runtime::RuntimeContext;
use horch_core::vcs::git::GitCli;

use super::cli::CleanupArgs;
use super::promote::find_round;
use super::{dataset_paths, exit};

pub(crate) fn cleanup(ctx: &RuntimeContext, args: &CleanupArgs) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let (round, view, _) = find_round(&paths, &args.round)?;
    match view.state {
        RoundState::Complete => {
            println!("round {round} is already COMPLETE");
            return Ok(exit::SUCCESS);
        }
        RoundState::NeedsIntervention if !args.force => {
            println!(
                "REFUSED: round {round} needs intervention; `cleanup --force` removes its worktrees"
            );
            return Ok(exit::NEEDS_INTERVENTION);
        }
        _ => {}
    }
    // A frozen worktree or a judge bundle can be read-only (0500):
    // `git worktree remove` needs to write in it.
    for c in view.candidates.values() {
        if let Some(wt) = &c.worktree {
            make_writable(&wt.path);
        }
    }
    let git = GitCli::new(ctx.bins.harness.git.clone());
    let recorder = JsonlRecorder::open(&paths, StoreOptions::from_faults(&ctx.settings.faults))?;
    let outcome = RoundCleanup {
        git: &git,
        recorder: &recorder,
        paths: &paths,
        faults: &ctx.settings.faults,
        repo: ctx.paths.project()?,
    }
    .run(
        &round,
        &view,
        CleanupOptions {
            prune_branches: args.prune_branches,
            force: args.force,
        },
    );
    match outcome {
        Ok(CleanupOutcome::Done { removed, failed }) => {
            println!(
                "round {round} COMPLETE: removed {} worktrees ({}){}",
                removed.len(),
                removed.join(", "),
                if failed.is_empty() {
                    String::new()
                } else {
                    format!("; could not remove {}", failed.join(", "))
                }
            );
            Ok(exit::SUCCESS)
        }
        Ok(CleanupOutcome::Skipped { reason }) => {
            println!("REFUSED: {reason}");
            Ok(exit::FAILURE)
        }
        Err(e) => {
            if let Some(f) = e.downcast_ref::<FaultFired>() {
                eprintln!("multi-herdr-dataset: {f}: aborting");
                std::process::exit(ABORT_EXIT_CODE);
            }
            Err(e)
        }
    }
}

/// Give the owner write permission on every directory under `dir`. Best
/// effort: a failure leaves the removal to fail and be recorded.
#[cfg(unix)]
fn make_writable(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&d) else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let mode = meta.permissions().mode();
        if mode & 0o700 != 0o700 {
            let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(mode | 0o700));
        }
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            stack.push(e.path());
        }
    }
}

#[cfg(not(unix))]
fn make_writable(_dir: &std::path::Path) {}
