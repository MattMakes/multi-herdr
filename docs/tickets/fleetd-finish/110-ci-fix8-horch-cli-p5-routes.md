# CI FIX-8: migrate stale CLI routing tests in p5_route_ledger_run.rs, p5_route_ledger_task.rs and p5_route_operator_commands.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --test p5_route_ledger_run --test p5_route_ledger_task --test p5_route_operator_commands --locked && cargo fmt --all --check`
**Own:** `crates/horch/tests/p5_route_ledger_run.rs`, `crates/horch/tests/p5_route_ledger_task.rs`, `crates/horch/tests/p5_route_operator_commands.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-8).

## Goal

Fix the 14 failing tests in `crates/horch/tests/p5_route_ledger_run.rs` (4 tests), `crates/horch/tests/p5_route_ledger_task.rs` (4 tests), and `crates/horch/tests/p5_route_operator_commands.rs` (6 tests) that fail because Phase 6 retired `horch ledger run new`, `horch ledger add`, `horch ledger task`, `horch ledger add-map` (without `--dry-run`), and `horch ledger worker new`, or because the test fixture inserts a duplicate `features.branch` row.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-8.
- `crates/horch/tests/p5_route_ledger_run.rs`, `crates/horch/tests/p5_route_ledger_task.rs`, and `crates/horch/tests/p5_route_operator_commands.rs`.
- `crates/horch/tests/p6_retired_run_commands.rs` and `crates/horch/tests/p6_retired_worker_commands.rs` for the Phase 6 retired-command contract.

## Tests covered (14 tests: 6 class `f` + 8 class `f?` caused by retired `horch ledger run new`)

1. `crates/horch/tests/p5_route_operator_commands.rs` (6 class `f` tests):
   - `each_operator_command_that_sets_horch_role_is_still_decided_by_fleetd` (`crates/horch/tests/p5_route_operator_commands.rs:119`): `Err: UNIQUE constraint failed: features.branch`
   - `feature_branch_and_rescue_commands_send_their_kinds` (`crates/horch/tests/p5_route_operator_commands.rs:119`): `Err: UNIQUE constraint failed: features.branch`
   - `job_requeue_sends_job_requeue` (`crates/horch/tests/p5_route_operator_commands.rs:119`): `Err: UNIQUE constraint failed: features.branch`
   - `task_requeue_and_cancel_send_their_kinds` (`crates/horch/tests/p5_route_operator_commands.rs:119`): `Err: UNIQUE constraint failed: features.branch`
   - `worktree_restore_and_triage_send_their_kinds` (`crates/horch/tests/p5_route_operator_commands.rs:119`): `Err: UNIQUE constraint failed: features.branch`
   - `worktree_triage_archive_sends_1_request_and_queues_1_job` (`crates/horch/tests/p5_route_operator_commands.rs:119`): `Err: UNIQUE constraint failed: features.branch`
2. `crates/horch/tests/p5_route_ledger_run.rs` (4 class `f?` tests):
   - `add_map_sends_paths_and_sha256_not_texts` (`crates/horch/tests/p5_route_ledger_run.rs:140`): `assert_eq failed: ["ledger", "run", "new", "r1"]` (`horch ledger run new is retired`)
   - `fleetd_down_queues_a_request_file` (`crates/horch/tests/p5_route_ledger_run.rs:140`): `assert_eq failed: ["ledger", "run", "new", "r1"]` (`horch ledger run new is retired`)
   - `run_new_sends_1_run_new_request` (`crates/horch/tests/p5_route_ledger_run.rs:140`): `assert_eq failed: ["ledger", "run", "new", "r1", "--check", "true"]` (`horch ledger run new is retired`)
   - `task_add_and_task_solo_send_their_kinds` (`crates/horch/tests/p5_route_ledger_run.rs:140`): `assert_eq failed: ["ledger", "run", "new", "r1"]` (`horch ledger run new is retired`)
3. `crates/horch/tests/p5_route_ledger_task.rs` (4 class `f?` tests):
   - `requeue_cancel_and_waive_send_their_kinds` (`crates/horch/tests/p5_route_ledger_task.rs:177`): `assert_eq failed: ["ledger", "run", "new", "r1"]` (`horch ledger run new is retired`)
   - `worker_that_removes_horch_role_is_refused_by_fleetd` (`crates/horch/tests/p5_route_ledger_task.rs:177`): `assert_eq failed: ["ledger", "run", "new", "r1"]` (`horch ledger run new is retired`)
   - `worker_that_sets_horch_role_orchestrator_is_refused_by_fleetd` (`crates/horch/tests/p5_route_ledger_task.rs:177`): `assert_eq failed: ["ledger", "run", "new", "r1"]` (`horch ledger run new is retired`)
   - `worktree_drop_sends_worktree_drop` (`crates/horch/tests/p5_route_ledger_task.rs:177`): `assert_eq failed: ["ledger", "run", "new", "r1"]` (`horch ledger run new is retired`)

## Steps

1. In `crates/horch/tests/p5_route_operator_commands.rs:119`, inspect the `World` setup where `runs::create` (which already creates a `features` row for the run branch under Phase 6) is followed by a manual `INSERT INTO features` with the same `branch`. Use `INSERT OR REPLACE` or a distinct feature branch / the feature created by `runs::create` + `schema::run_idea` so all 6 tests pass without `UNIQUE constraint failed: features.branch`.
2. In `crates/horch/tests/p5_route_ledger_task.rs`, replace `self.ok(&["ledger", "run", "new", "r1"])`, `["ledger", "add", ...]`, and `["ledger", "worker", "new", ...]` in `run_with_task()` and `worktree_drop_sends_worktree_drop()` with direct `Store` seeding (`runs::create`, `schema::run_idea`, `tasks::add`, `workers::insert`).
3. In `crates/horch/tests/p5_route_ledger_run.rs`:
   - For `fleetd_down_queues_a_request_file`, route a kept command (such as `horch ledger import` or `horch ledger gc` or `horch idea add`) when fleetd is down, or if `p5_route_ledger_task.rs` already covers `fleetd_down_queues_a_request_file` and the 3 tests `run_new_sends_1_run_new_request`, `add_map_sends_paths_and_sha256_not_texts`, and `task_add_and_task_solo_send_their_kinds` test CLI subcommands that Phase 6 retired on purpose (`p6_retired_run_commands.rs`), delete the tests of the retired commands and write the reason and design rule (`FR-12`, Phase 6 ticket 47 retired `run new`, `add`, `task`, `add-map`) in the commit message. Never weaken an assert.
4. Run the Check:
   `scripts/dev-sandbox.sh cargo test -p horch --test p5_route_ledger_run --test p5_route_ledger_task --test p5_route_operator_commands --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `**Own:**`.

## Done when

All tests in `p5_route_ledger_run.rs`, `p5_route_ledger_task.rs`, and `p5_route_operator_commands.rs` pass, and the Check passes.

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
