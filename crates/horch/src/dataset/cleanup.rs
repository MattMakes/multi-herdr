//! `multi-herdr-dataset cleanup <round> [--force] [--prune-branches]`: the
//! operator removes a round's worktrees (dataset design §5.1, PRO-07).
//!
//! The coordinator cleans up a round at its end. This command is for the
//! rest: a round in NEEDS_INTERVENTION (only with `--force`, exit 5
//! without), or a cleanup that a crash stopped (CLEANUP). Branches stay
//! unless `--prune-branches`. A worktree that cannot be removed is
//! recorded as `worktree.cleanup_failed`.
//!
//! A COMPLETE round leaves no empty directory: [`remove_empty_dirs`] takes
//! away the round's `<root>/<experiment>/_promote/` and
//! `<root>/<experiment>/` when nothing is left in them.

use std::path::{Path, PathBuf};

use anyhow::Result;
use horch_core::competition::cleanup::{CleanupOptions, CleanupOutcome, RoundCleanup};
use horch_core::competition::model::RoundState;
use horch_core::competition::promotion::FaultFired;
use horch_core::measure::projection::RoundView;
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
            remove_empty_dirs(&view);
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

/// Remove the directories a round's worktrees lived in, when they are
/// empty: `<root>/<experiment>/_promote/`, then `<root>/<experiment>/`.
/// Never recursive: a directory with anything left in it stays. A directory
/// whose name is not the experiment id is not one the run created, and
/// stays.
pub(crate) fn remove_empty_dirs(view: &RoundView) {
    let worktrees = view
        .candidates
        .values()
        .filter_map(|c| c.worktree.as_ref().map(|w| w.path.as_path()));
    remove_empty_parents(worktrees, view.experiment_id.as_str());
}

/// [`remove_empty_dirs`] on the worktree paths of experiment `experiment`.
fn remove_empty_parents<'a>(worktrees: impl Iterator<Item = &'a Path>, experiment: &str) {
    let mut dirs: Vec<PathBuf> = worktrees
        .filter_map(Path::parent)
        .filter(|d| d.file_name().is_some_and(|n| n == experiment))
        .map(Path::to_path_buf)
        .collect();
    dirs.sort();
    dirs.dedup();
    for dir in dirs {
        let _ = std::fs::remove_dir(dir.join("_promote"));
        let _ = std::fs::remove_dir(&dir);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// F7: an empty `_promote/` and an empty experiment dir go; a dir with
    /// a file left in it, or not named for the experiment, stays.
    #[test]
    fn cleanup_removes_only_empty_round_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Experiment e1: every worktree is gone, `_promote/` is empty.
        let e1 = root.join("e1");
        std::fs::create_dir_all(e1.join("_promote")).unwrap();
        // Experiment e2: one worktree could not be removed.
        let e2 = root.join("e2");
        std::fs::create_dir_all(e2.join("B")).unwrap();
        std::fs::write(e2.join("B/kept.txt"), "x").unwrap();
        // A worktree root not named for its experiment.
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();

        remove_empty_parents(
            [e1.join("A"), e1.join("B")].iter().map(PathBuf::as_path),
            "e1",
        );
        remove_empty_parents(
            [e2.join("A"), e2.join("B")].iter().map(PathBuf::as_path),
            "e2",
        );
        remove_empty_parents([other.join("A")].iter().map(PathBuf::as_path), "e3");

        assert!(!e1.exists(), "empty experiment dir stays");
        assert!(e2.join("B/kept.txt").is_file(), "non-empty dir removed");
        assert!(other.is_dir(), "a dir not named for the experiment went");
        assert!(root.is_dir());
    }
}
