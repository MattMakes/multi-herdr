# CI FIX-8: migrate stale tests in horch-core stop.rs, context_prime.rs, stack_edges.rs and worktree_disk.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib taskledger::stop::tests --locked && scripts/dev-sandbox.sh cargo test -p horch-core --test context_prime --test stack_edges --test worktree_disk --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/stop.rs`, `crates/horch-core/tests/context_prime.rs`, `crates/horch-core/tests/stack_edges.rs`, `crates/horch-core/tests/worktree_disk.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-8).

## Goal

Fix the 7 class `f` tests in `horch-core` (`stop.rs`, `context_prime.rs`, `stack_edges.rs`, and `worktree_disk.rs`) that fail on macOS and Linux because their fixtures do not seed an idea in flight for the run (`FR-12`) or an operator token for the routed command channel.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` (`schema::run_idea(tx, &run.run_id, &util::now(), true)`).
- The 4 files in `**Own:**` at the lines listed below.

## Tests covered (7 class `f` rows)

1. `crates/horch-core/src/taskledger/stop.rs` (2 tests):
   - `taskledger::stop::tests::trf_36_a_cancel_stops_the_worker_by_its_pids_and_records_each_stop` (`crates/horch-core/src/taskledger/stop.rs:1043`): `Err: run s has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add`
   - `taskledger::stop::tests::trf_36_a_worker_that_cannot_be_stopped_leaves_the_task_and_names_the_pid` (`crates/horch-core/src/taskledger/stop.rs:1043`): `Err: run s has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add`
2. `crates/horch-core/tests/context_prime.rs` (1 test):
   - `ctx_05_prime_launch_records_the_applied_window` (`crates/horch-core/tests/context_prime.rs:503`): `Err: reading the operator token`
3. `crates/horch-core/tests/stack_edges.rs` (1 test):
   - `both_gates_run_again_after_the_rebase` (`crates/horch-core/tests/stack_edges.rs:565`): `Err: run F-0002 has no idea in flight of its feature F-0002, and a task needs an idea (FR-12). Add one: horch idea add`
4. `crates/horch-core/tests/worktree_disk.rs` (3 tests):
   - `a_tree_whose_removal_failed_frees_no_space` (`crates/horch-core/tests/worktree_disk.rs:191`): `Err: run r1 has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add`
   - `over_budget_never_removes_an_active_tree_and_holds_dispatch` (`crates/horch-core/tests/worktree_disk.rs:191`): `Err: run r1 has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add`
   - `over_budget_removes_landed_and_rescued_trees_of_finished_runs_oldest_first` (`crates/horch-core/tests/worktree_disk.rs:191`): `Err: run r1 has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add`

## Steps

1. In `crates/horch-core/src/taskledger/stop.rs` (around line 1043), in the test fixture that creates run `s` before calling `tasks::add`, call `crate::taskledger::schema::run_idea(tx, &run.run_id, &crate::taskledger::util::now(), true)?` so the run has an approved idea in flight under FR-12.
2. In `crates/horch-core/tests/stack_edges.rs` (around line 565) and `crates/horch-core/tests/worktree_disk.rs` (around line 191), update the run-creation helper to seed `schema::run_idea(tx, &run.run_id, &util::now(), true)` for each created run before `tasks::add`.
3. In `crates/horch-core/tests/context_prime.rs` (around line 503), seed the operator token file at `<fleet_root>/run/operator.token` (mode `0o600`, or use a local store/writer fixture in the test) so `ctx_05_prime_launch_records_the_applied_window` does not fail with `reading the operator token`.
4. Never weaken an assert. Run the Check:
   `scripts/dev-sandbox.sh cargo test -p horch-core --lib taskledger::stop::tests --locked && scripts/dev-sandbox.sh cargo test -p horch-core --test context_prime --test stack_edges --test worktree_disk --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `**Own:**`.

## Done when

All 7 tests in `stop.rs`, `context_prime.rs`, `stack_edges.rs`, and `worktree_disk.rs` pass, and the Check passes.

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
