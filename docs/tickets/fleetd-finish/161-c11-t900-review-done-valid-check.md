# 14: C11 T-900: complete code-sweep review tasks of a push attempt and make taskledger.txt empty

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked taskledger::finish && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked fleetd::task_execution && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test taskledger -- the_whole_flow_on_the_fake_herdr stopping_one_of_three_workers_leaves_the_others_running a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/resultcmd.rs`, `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/src/fleetd/task_execution/application/review.rs`, `crates/horch-core/src/fleetd/task_execution/application/ingest.rs`, `crates/horch-e2e/expected-failures/taskledger.txt`, `crates/horch-e2e/tests/taskledger.rs`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (section 1 items L6, L13; section 3 unit C11) and `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/unlanded-branches.md` (section "T-900 - complete push code-sweep review tasks").

## Read first: the original ticket of this task

The original ticket of the old run is in your worktree: `ai_docs/plans/fleetd-build/tickets/62-review-task-of-a-push-attempt.md`. Read it before the steps below. It is the source of truth for the intent, the fixes (for example "fix A") and the tests. When it and this ticket do not agree on the product behavior, the original ticket wins; send a NOTE that names the difference. The rules of this ticket about the sandbox, the Check and the merge still apply. The notes of the old run are in `ai_docs/plans/fleetd-build/notes/` (find your task with `ls ai_docs/plans/fleetd-build/notes | grep -i <T-number>`).

## Goal

Give a code-sweep review task (`task.is_review()`) of a push attempt a valid path from `horch result` through the verifier to `done` without requiring its own worktree, remove `the_whole_flow_on_the_fake_herdr` from `crates/horch-e2e/expected-failures/taskledger.txt`, and prove that the test passes 3 times in a row.

## Requirements

- **L6 / L13 (T-900)**:
  1. In `crates/horch/src/cmd/resultcmd.rs` (`write` and `worktree_of` around lines 145 to 177): a review task (`task.is_review()`, or a task with `parent_task_id` whose kind/mode is a code-sweep review) has no worktree of its own and produces a review report file rather than branch commits. Allow `horch result` on a review task without `--commit` when the task has no worktree row (recording empty `commits` or reading the parent task's worktree/commits).
  2. In `crates/horch-core/src/fleetd/task_execution/application/verifier.rs` (around line 408) and `crates/horch-core/src/fleetd/task_execution/application/ingest.rs` / `review.rs`: when a code-sweep review task of a push attempt writes `result.json` with `status: completed` (or reports done), transition the review task and its attempt to `done` (as `finish/mod.rs:745` notes that a review task has no finish steps of its own) so that the parent task's `code-sweep` step in `review::code_sweep` sees `open_reviews == 0` and advances.
  3. Remove `the_whole_flow_on_the_fake_herdr` from `crates/horch-e2e/expected-failures/taskledger.txt` (line 5) so that `taskledger.txt` contains only comments.
  4. Run `the_whole_flow_on_the_fake_herdr` 3 times (rule 7).

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Inspect `crates/horch-e2e/tests/taskledger.rs` (`the_whole_flow_on_the_fake_herdr`), `crates/horch/src/cmd/resultcmd.rs:132-178`, `crates/horch-core/src/taskledger/finish/mod.rs:743-750`, `crates/horch-core/src/fleetd/task_execution/application/ingest.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:408-455`, and `crates/horch-core/src/fleetd/task_execution/application/review.rs:395-410`.
   - Check: write a unit test in `horch-core` for a push-dispatched code-sweep review task completing `result.json` and reaching `TaskStatus::Done`, and see it fail.
2. Update `crates/horch/src/cmd/resultcmd.rs` so that when the attempt's task is a review task (`task.is_review()`) and has no worktree row in `worktrees`, `horch result` succeeds with empty `commits` and `files_changed` unless `--commit` or `--file` is given.
   - Check: `scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::resultcmd`
3. Update `crates/horch-core/src/fleetd/task_execution/application/ingest.rs`, `verifier.rs`, and `review.rs` (and `crates/horch-core/src/taskledger/finish/mod.rs` if needed by post-review aggregation) so that a code-sweep review task of a push attempt has no finish steps or worktree check, moves to `done` with its attempt ended `verified` / `done` once its `result.json` (or done outbox file) is ingested, and unblocks its parent task's `code-sweep` step in `verifying`.
   - Check: `scripts/dev-sandbox.sh cargo test -p horch-core --lib taskledger::finish fleetd::task_execution`
4. Remove `the_whole_flow_on_the_fake_herdr` from `crates/horch-e2e/expected-failures/taskledger.txt`. Build the workspace binaries and run `the_whole_flow_on_the_fake_herdr` 3 times (rule 7), then run the full `taskledger` e2e binary:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked`
   `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-e2e --test taskledger -- the_whole_flow_on_the_fake_herdr || exit 1; done`
   `scripts/dev-sandbox.sh cargo test -p horch-e2e --test taskledger`
   - Check: `grep -v "^#" crates/horch-e2e/expected-failures/taskledger.txt` prints nothing, and all 3 runs pass.
5. Run the ticket Check.

## Out of scope

- Migrating `fleet18_observability.rs` tests (unit C12 / T-882).

## Done when

`crates/horch-e2e/expected-failures/taskledger.txt` has no active entry, `the_whole_flow_on_the_fake_herdr` passes 3 times in a row, and the Check passes.

## Added 2026-11-20: build of the whole workspace before the land

This run changed about 60 test files after the task branch was made. Before you send READY-TO-LAND: run `scripts/dev-sandbox.sh cargo test --workspace --locked --no-run` 1 time. If a test file outside your Own does not build because of your merge (for example a function that your branch removes), tell the orchestrator the file; it is then added to your Own. Do not land a unit that breaks the build of another test file.


## Added 2026-11-20: this ticket replaces ticket 14

The Check of ticket 14 gave cargo 2 test filters before `--`, which cargo refuses. This ticket has the same work with a valid Check (1 filter in each cargo command).


## Added 2026-11-20: the file taskledger.rs is red on the base

61 tests of `crates/horch-e2e/tests/taskledger.rs` are red on the base (they call the retired `horch ledger run new`; tasks T-90 to T-93 own them). Do not run the full `taskledger` binary as a gate and do not fix those tests: the Check runs only `the_whole_flow_on_the_fake_herdr` and the 2 tests of T-893. Where a step of this ticket says "run the full taskledger e2e binary", run the Check in its place. Landing: when the Check passes, do NOT run `horch ledger finish`; send `NOTE: READY-TO-LAND` and wait for GO; then fetch, `git rebase --rebase-merges` on `fleet/fleetd-finish`, and finish.
