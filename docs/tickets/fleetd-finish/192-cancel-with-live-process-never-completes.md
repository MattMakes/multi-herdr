# A cancel of a running task never completes: the task stays `running` with no lease and no live attempt (I1)

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test cancel_completes && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test taskledger -- a_cancelled_task_does_not_land_or_turn_done a_cancel_during_the_check_lands_nothing && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p5_push_stop && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/cancel.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/tests/cancel_completes.rs`, `crates/horch-e2e/tests/taskledger.rs`, `crates/horch-e2e/expected-failures/taskledger.txt`

## Fault

Found by task T-234 (qa-engineer-55) on 2026-11-20.

- A task has a live attempt with a live agent process. `horch ledger cancel T-1` prints `T-1 is not cancelled yet: a process of its attempt still runs`.
- The first cancel step ends the attempt and drops the lease.
- The pass then reports `I1: task is leased or running, but each of its attempts ended and it has no lease. No pass rule moves it`. The task stays `running` for more than 120 s; it never reaches `cancelled`.
- Seen with a plain sleep agent in its own process group, a sh agent, and an agent that already finished.
- Code (from the worker): `complete()` in `crates/horch-core/src/fleetd/task_execution/application/cancel.rs` needs the process of each attempt seen dead, and the pass reads only the processes of the attempts that are not ended. So after the attempt is ended, no one sees its process die.
- The tests `a_cancelled_task_does_not_land_or_turn_done` and `a_cancel_during_the_check_lands_nothing` of `crates/horch-e2e/tests/taskledger.rs` are on `crates/horch-e2e/expected-failures/taskledger.txt` with this ticket as owner.

Related: task T-195 fixed another path to the same I1 state (a launch timeout). Read its commit (`git log --grep T-195`). The design: a cancel from each task state has an exit (A34), and a dev fleetd never stops a process that it did not start; the stop ladder of an attempt is in the design section of the worker stop.

## Goal

A cancel of a task with a live attempt reaches `cancelled` in bounded time: fleetd stops the processes of the attempt by the stop ladder, sees them dead, and completes the cancel. No task stays in the I1 state after a cancel. First decide with evidence if the fault is in the product or only in the bind helper of the test (other cancel tests pass: `p5_push_stop.rs`, `p6_judge_pair.rs`): say what differs.

## Steps

1. Write the failing test `crates/horch-core/tests/cancel_completes.rs`: a running task, an attempt with a live process, a cancel; after the passes the task is `cancelled` and the pass reports no I1 item. Add the case "the agent already finished".
2. Fix. Then remove the 2 entries from `expected-failures/taskledger.txt` and the `expected::check` of the 2 tests; run them 3 times.
3. If the fix needs a file outside Own, send `QUESTION:` with the file.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. No flaky test. Do not weaken an assert. No `#[ignore]`.
6. Never run `rm -rf /tmp/horch-e2e-*`. Remove only the exact folder of your own test.
7. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes, the 2 entries are gone, and the 2 tests pass 3 times in a row.


## Added 2026-11-21T02:35Z: shared files (read this first)

To let tasks run at the same time, the ledger Own list of this task does not hold each file that the ticket names. A file that the ticket text names is still in your scope. Other tasks edit other tests or functions of the same files at the same time: `crates/horch-e2e/tests/p3_refusals.rs` and its expected-failures list (tickets 164, 166, 168), `crates/horch/src/cmd/taskledger.rs` (tickets 166, 167), `crates/horch-e2e/tests/p3_unlanded.rs` (tickets 167, 168), `crates/horch-e2e/tests/taskledger.rs` and its list (tickets 168, 192), `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` (tickets 42, 73, 192). Rules: change only the tests and functions of your ticket in a shared file; do not reformat or move other code there; before `horch ledger finish` and again when the reviews are in, run `git fetch` and `git rebase fleet/fleetd-finish` and resolve a conflict in favor of both changes; then run the Check again.

The host has a high load from another fleet of the operator (not ours; never touch its processes). A test that fails only by a time limit under load: send `NOTE:` with the test and the line, and run 1 more time. Do not make a time limit larger without a measured cause. Write 1 `horch ledger progress` line each 5 minutes while a long test runs. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall.
