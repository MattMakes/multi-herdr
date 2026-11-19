# The same-second tests of the worktree reconciler do not depend on the wall clock

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-core --test worktree_reconcile && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/worktree_reconcile.rs`

## Goal

Each test in `crates/horch-core/tests/worktree_reconcile.rs` gives the same result on a slow or
busy machine. No test needs 2 real job runs to end in the same wall-clock second.

## Context

Task T-300 made the stale-list rule of the reconciler use the order of the job ids, not the 1 s
timestamps, and added 2 tests for the 2 orders in the same second. The test
`a_list_that_ended_before_a_removal_in_the_same_second_adopts_nothing` (about line 835) has a
guard that fails when the 2 job runs end in 2 different seconds. Under load that occurs, and the
finish check of other tasks fails (T-535 saw it). The machine runs about 20 workers, so the load
is high all the time.

## Steps

1. Read the 2 same-second tests and their guard.
2. Make the time exact: write the 2 job rows (the `worktree_list` run and the `worktree_remove`
   run) with the same `ended_at` text by the fixture, or set the time through the clock seam that
   the other tests of the file use. Do not sleep and do not retry until the seconds agree.
3. Keep what each test proves: with the same second, the order of the job ids decides (a list that
   ended before the removal is stale and adopts nothing; a list that started after the removal is
   fresh and adopts).
4. Run the 2 tests 30 times in a loop while another build runs. Each run passes.

## Constraints

- Test code only. No product change. If the test cannot be exact with no product change, send
  QUESTION.
- Simplified Technical English. No em dash.

## Done when

- The Check passes, and the done summary gives the result of the 30 runs.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
