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
use horch_core::ids::RoundId;
use horch_core::measure::recorder::JsonlRecorder;
use horch_core::measure::store::StoreOptions;
use horch_core::runtime::RuntimeContext;
use horch_core::vcs::git::GitCli;

use super::cli::CleanupArgs;
use super::promote::find_round;
use super::{abort_on_fault, dataset_paths, exit};

pub(crate) fn cleanup(ctx: &RuntimeContext, args: &CleanupArgs) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let (round, view, _) = find_round(&paths, &args.round)?;
    if let Some((line, code)) = refusal(&round, view.state, args.force) {
        println!("{line}");
        return Ok(code);
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
        clock: &horch_core::clock::now,
    }
    .run(
        &round,
        &view,
        CleanupOptions {
            prune_branches: args.prune_branches,
            force: args.force,
        },
    );
    report(&round, outcome)
}

/// The line and exit code that refuse a cleanup, or `None` when it may run.
/// A COMPLETE round has nothing to remove. A round that needs intervention
/// needs `--force`.
fn refusal(round: &RoundId, state: RoundState, force: bool) -> Option<(String, u8)> {
    match state {
        RoundState::Complete => Some((format!("round {round} is already COMPLETE"), exit::SUCCESS)),
        RoundState::NeedsIntervention if !force => Some((
            format!(
                "REFUSED: round {round} needs intervention; `cleanup --force` removes its worktrees"
            ),
            exit::NEEDS_INTERVENTION,
        )),
        _ => None,
    }
}

/// Print the outcome of the cleanup and give its exit code. An injected
/// fault aborts the process.
fn report(round: &RoundId, outcome: Result<CleanupOutcome>) -> Result<u8> {
    match outcome {
        Ok(CleanupOutcome::Done { removed, failed }) => {
            println!(
                "round {round} COMPLETE: removed {} worktrees ({}){}",
                removed.len(),
                removed.join(", "),
                failed_suffix(&failed)
            );
            Ok(exit::SUCCESS)
        }
        Ok(CleanupOutcome::Skipped { reason }) => {
            println!("REFUSED: {reason}");
            Ok(exit::FAILURE)
        }
        Err(e) => {
            abort_on_fault(&e);
            Err(e)
        }
    }
}

/// The report text that names the worktrees that could not be removed.
fn failed_suffix(failed: &[String]) -> String {
    if failed.is_empty() {
        String::new()
    } else {
        format!("; could not remove {}", failed.join(", "))
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

    fn r1() -> RoundId {
        RoundId::new("r1").unwrap()
    }

    #[test]
    fn a_complete_round_is_refused_with_success_even_with_force() {
        for force in [false, true] {
            assert_eq!(
                refusal(&r1(), RoundState::Complete, force),
                Some(("round r1 is already COMPLETE".to_string(), 0))
            );
        }
    }

    #[test]
    fn a_round_that_needs_intervention_is_refused_without_force() {
        assert_eq!(
            refusal(&r1(), RoundState::NeedsIntervention, false),
            Some((
                "REFUSED: round r1 needs intervention; `cleanup --force` removes its worktrees"
                    .to_string(),
                5
            ))
        );
    }

    #[test]
    fn a_round_that_needs_intervention_runs_with_force() {
        assert_eq!(refusal(&r1(), RoundState::NeedsIntervention, true), None);
    }

    #[test]
    fn a_round_in_cleanup_state_runs() {
        assert_eq!(refusal(&r1(), RoundState::Cleanup, false), None);
    }

    #[test]
    fn a_skipped_cleanup_reports_the_reason_and_fails() {
        let code = report(
            &r1(),
            Ok(CleanupOutcome::Skipped {
                reason: "no worktrees".into(),
            }),
        )
        .unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn a_done_cleanup_succeeds_even_with_a_failed_removal() {
        let code = report(
            &r1(),
            Ok(CleanupOutcome::Done {
                removed: vec!["a".into()],
                failed: vec!["b".into()],
            }),
        )
        .unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn an_ordinary_error_is_returned_not_aborted() {
        let err = report(&r1(), Err(anyhow::anyhow!("boom"))).unwrap_err();
        assert_eq!(err.to_string(), "boom");
    }

    #[test]
    fn failed_suffix_names_the_worktrees_or_is_empty() {
        assert_eq!(failed_suffix(&[]), "");
        assert_eq!(
            failed_suffix(&["a".into(), "b".into()]),
            "; could not remove a, b"
        );
    }
}
