# 9 more loops of worktree custody read the store for each item (the pass rule of T-292)

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan --test worktree_reconcile --test worktree_reconcile_acting --test gate_jobs && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked worktree_custody && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/disk.rs`

## Fault

The commit of review finding D244-F2 (`git log --grep 'D244-F2' -1` on the run branch; task T-289) lists 9 loops that it did not change, each with a store read for each item: in `reconcile.rs` `check_i6`, `observe_runs`, `stale`, `escalate_vanished`, `act`, `idea_merged`, `given_up`; in `disk.rs` `queue_removals`; and `queries.rs`, `rescue_refs.rs`. Rule: in 1 pass no SQL statement for each row; a whole step reads in a fixed number of statements on 1 snapshot, through the reader of the context that owns the table; proof by a statement count (`taskledger::testing::count_statements`).

## Steps

1. For each of the 9: is it on the path of a pass (then fix), or a CLI read of 1 item (then leave it and say so)? 2. Fix each pass loop in the way of the D244-F2 commit (set reads into maps on the snapshot). Same results for each edge: keep the tests green and add a statement-count test with 3 and 300 rows for each fixed step. 3. `queries.rs` and `rescue_refs.rs` are outside your Own: send `QUESTION:` with the lines if they need a change.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install a tool or the build.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. No OS `cfg` outside `horch-platform`. No flaky test, no `#[ignore]`, no weaker assert. A pass runs no SQL statement for each row and never waits for a job.
4. Never run pkill or killall. Never touch a process of another fleet. `gh` is read only: never rerun, cancel or change a run or a pull request.
5. Write 1 `horch ledger progress` line each 5 minutes inside each wait loop (the watchdog stops a silent worker after 30 minutes). Commit after each step.
6. `git fetch` and `git rebase fleet/fleetd-finish` before `horch ledger finish`. After the comment sweep, run finish again at once; restore no comment.
7. Before DONE: no process of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes and the commit message names each cause with its proof.
