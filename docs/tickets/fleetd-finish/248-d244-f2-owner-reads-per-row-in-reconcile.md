# Review finding D244-F2 (Important): the reconciler reads the owner of each active tree with SQL statements for each row

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan --test worktree_reconcile --test gate_jobs && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked worktree_custody && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`

Source: `docs/specs/fleetd/reviews/delta-review-custody-and-conformance.md` on the run branch. Read the finding first.

## Fault

In `worktree_custody/application/reconcile.rs` (about line 577) the pass iterates over each active worktree row and `owner()` calls `state_of()` for the attempt, `workers::find()` for the worker and `tasks::get()` for the task. The count of SQL statements grows with the count of active trees. Rule of this run (task T-292): in 1 pass no SQL statement for each row; whole steps read in a fixed number of statements, on 1 snapshot; proof by a statement count (`taskledger::testing::count_statements`).

## Steps

1. `git fetch` and rebase first: tasks T-327 and T-289 changed this file (the snapshot `one_snapshot`, the readers of the owner contexts, the removal of a released or dropped tree). Check with `ls crates/horch-core/tests | grep -i reconcile` which test file holds the reconcile tests, and if the name in the Check is wrong, send `QUESTION:` at once with the right name (the Check of a queued task cannot change).
2. Read the attempt states, the worker statuses, the task states and the parked flags of all active rows in bounded set reads (a fixed number of statements, paged by the page size of the pass if the code has one) inside the same snapshot, through the readers of the contexts that own the tables (`fitness_no_context_touches_foreign_tables` stays green). `owner()` reads only those maps.
3. The result is the same as before for each edge: no attempt row, no worker row, no task row, an ended attempt with no process (task T-289: not live), an attempt that is not ended and records no process (unknown: the removal is refused). Keep or add a test for each.
4. Proof: a statement-count test with 3 and with 300 active rows gives the same count. It fails before the fix. Then search the other steps of `act` and the other passes of `worktree_custody/application` for the same pattern (a loop over rows with a store call inside) and list each in the commit message: fixed here, or named with its line.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`, or the ledger of another fleet. Never install the build.
3. A pass never waits for a job and runs no SQL statement for each row; a row is written before its effect; no OS `cfg` outside `horch-platform`. No flaky test, no `#[ignore]`, no weaker assert, no expected-failures entry.
4. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Never touch a process of another fleet. At most 4 test processes at the same time. 1 `horch ledger progress` line each 5 minutes while a long test runs.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit and restore each comment that gives a security rule, an invariant, a bound or a design id.
6. Before DONE: no process of the unit is alive (`ps`), and the sandbox folder is removed.

## Done when

The Check passes and the statement-count test fails before the fix.
