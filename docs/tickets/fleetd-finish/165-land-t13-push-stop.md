# Land the work of task T-13 (unit C10, T-893: stop and stall of a push attempt) with a Check that has no test that is red on the base

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p5_push_stop && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test taskledger -- stopping_one_of_three_workers_leaves_the_others_running a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid the_whole_flow_on_the_fake_herdr && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`, `crates/horch-core/src/fleetd/task_execution/application/commands.rs`, `crates/horch-core/src/taskledger/stop.rs`, `crates/horch-e2e/expected-failures/taskledger.txt`, `crates/horch-e2e/tests/p5_push_stop.rs`, `crates/horch-e2e/tests/taskledger.rs`

## Goal

Task T-13 (ticket `13-c10-land-t893.md`) merged the branch of T-893 with `--no-ff` (commit 8f3bf5b8). Its Check ran the whole file `crates/horch-e2e/tests/taskledger.rs`, where 61 tests are red on the base because they call the retired `horch ledger run new` (tasks T-90 to T-93 own them and come after the chain). The local branch `keep/fleetd-finish-T13` (tip 8f3bf5b8) holds the work. This task lands it.

## Steps

1. In your worktree: `git merge --ff-only keep/fleetd-finish-T13`. If that fails because the run branch moved: `git reset --hard keep/fleetd-finish-T13`, then `git rebase --rebase-merges fleet/fleetd-finish`. The merge commit and the commits of T-893 must stay (`git log --merges`).
2. Read ticket `13-c10-land-t893.md` and do each step of it that the merge commit did not do yet (the worker of T-13 reported: p5_push_stop 3 of 3 green; the 3 tests of taskledger.rs of T-893 green 3 of 3; `the_whole_flow_on_the_fake_herdr` stays on its list).
3. Do not fix the other tests of `taskledger.rs`.
4. Run `scripts/dev-sandbox.sh cargo test --workspace --locked --no-run` 1 time. Run the Check 3 times.
5. Landing: when the Check passes, do NOT run `horch ledger finish`. Send `NOTE: READY-TO-LAND` with your task number to the orchestrator and wait for GO. Then `git fetch`, `git rebase --rebase-merges` on `fleet/fleetd-finish`, and run `horch ledger finish` at once. A finish run stops at the comment sweep; when the reviews are in, rebase again the same way and run finish again.

## Out of scope

New product changes.

## Done when

The work of T-13 is on the run branch with its merge commit and the Check passes.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

