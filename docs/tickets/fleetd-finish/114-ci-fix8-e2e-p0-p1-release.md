# CI FIX-8: migrate stale tests in horch-e2e p0_dispatch.rs, p0_landing.rs, p0_liveness.rs, p0_rescue.rs, p0_supervision.rs and p1_release.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p0_dispatch --test p0_landing --test p0_liveness --test p0_rescue --test p0_supervision --test p1_release --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p0_dispatch.rs`, `crates/horch-e2e/tests/p0_landing.rs`, `crates/horch-e2e/tests/p0_liveness.rs`, `crates/horch-e2e/tests/p0_rescue.rs`, `crates/horch-e2e/tests/p0_supervision.rs`, `crates/horch-e2e/tests/p1_release.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows 89-90, 92-102, 109-110).

## Goal

Migrate the 14 stale tests in `crates/horch-e2e/tests/p0_dispatch.rs` (2 tests), `crates/horch-e2e/tests/p0_landing.rs` (1 test), `crates/horch-e2e/tests/p0_liveness.rs` (4 tests), `crates/horch-e2e/tests/p0_rescue.rs` (3 tests), `crates/horch-e2e/tests/p0_supervision.rs` (2 tests), and `crates/horch-e2e/tests/p1_release.rs` (2 tests) from the retired `horch ledger run new` / `horch ledger add` CLI commands to `horch_e2e::ledger_seed`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.
- The 6 test files in `**Own:**`.

## Tests covered (14 rows of `red-baseline.txt`)

1. `crates/horch-e2e/tests/p0_dispatch.rs` (2 tests):
   - `fm2_agent_that_never_claims_has_its_task_bound_at_spawn`: `horch ledger run new alpha: status: exit status: 1 stdout:`
   - `fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt`: `horch ledger run new beta --check-every 1 --stall-rounds 1: status: exit status: 1 stdout:`
2. `crates/horch-e2e/tests/p0_landing.rs` (1 test; note that `two_conflicting_features_keep_their_tips_and_reach_a_decision` was already fixed in `28be192d`):
   - `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
3. `crates/horch-e2e/tests/p0_liveness.rs` (4 tests):
   - `fm3_idle_claim_is_caught_in_under_10_min`: `horch ledger run new eps: status: exit status: 1 stdout:`
   - `fm4_dead_agent_ends_its_attempt_within_30_s`: `horch ledger run new eps: status: exit status: 1 stdout:`
   - `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt`: `horch ledger run new eps: status: exit status: 1 stdout:`
   - `sleep_does_not_expire_a_lease`: `horch ledger run new eps: status: exit status: 1 stdout:`
4. `crates/horch-e2e/tests/p0_rescue.rs` (3 tests):
   - `fm13_herdr_restart_ends_attempts_and_queues_tasks_again`: `horch ledger run new fm13: status: exit status: 1 stdout:`
   - `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it`: `horch ledger run new fm13: status: exit status: 1 stdout:`
   - `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing`: `horch ledger run new fm25 --stall-rounds 1: status: exit status: 1 stdout:`
5. `crates/horch-e2e/tests/p0_supervision.rs` (2 tests):
   - `fm34_daemon_restart_on_a_new_binary_keeps_the_run`: `horch ledger run new zeta --check-every 1 --stall-rounds 1000: status: exit status: 1 stdout:`
   - `fm7_killed_watchdog_does_not_stop_the_reconcile`: `horch ledger run new zeta --check-every 1 --stall-rounds 1000: status: exit status: 1 stdout:`
6. `crates/horch-e2e/tests/p1_release.rs` (2 tests):
   - `fleetd_runs_no_pass_while_an_old_watchdog_may_still_run`: `horch ledger run new zeta --check-every 1 --stall-rounds 1000: status: exit status: 1 stdout:`
   - `update_with_a_live_worker_and_a_live_old_watchdog_leaves_1_control_path`: `horch ledger run new zeta --check-every 1 --stall-rounds 1000: status: exit status: 1 stdout:`

## Steps

1. In each of the 6 files in `**Own:**`, replace the `horch ledger run new` and `horch ledger add` CLI calls with `horch_e2e::ledger_seed::{seed_run, seed_task}` (or `Store::open` with `runs::create`, `schema::run_idea`, and `tasks::add` when custom run thresholds or steps like `--steps land`, `--check-every`, or `--stall-rounds` are set).
2. Never weaken an assert. Build the workspace binaries and run the Check:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p0_dispatch --test p0_landing --test p0_liveness --test p0_rescue --test p0_supervision --test p1_release --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `**Own:**`.

## Done when

All tests in `p0_dispatch.rs`, `p0_landing.rs`, `p0_liveness.rs`, `p0_rescue.rs`, `p0_supervision.rs`, and `p1_release.rs` pass, and the Check passes.

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
