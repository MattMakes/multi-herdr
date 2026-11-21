# 05: C2 T-899: remove operator-class requests and operator token access from the worker wrapper

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib execution && scripts/dev-sandbox.sh cargo test -p horch-core --test p5_operator_boundary --test p6_authorize --test p7_operator_boundary && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p5_operator_boundary && cargo fmt --all --check`
**Own:** `crates/horch-core/src/execution/lifecycle.rs`, `crates/horch-core/src/execution/service.rs`, `crates/horch-core/src/execution/store.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`, `crates/horch-core/src/fleetd/task_execution/domain/task_files.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/task_files.rs`, `crates/horch-core/src/fleetd/task_execution/application/ingest.rs`, `crates/horch-core/tests/p6_authorize.rs`, `crates/horch-e2e/tests/p5_operator_boundary.rs`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (section 1 item L5; section 3 unit C2), `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/unlanded-branches.md` (section "T-899 - remove operator-class requests from worker wrappers"), and `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/phases-0-P-1.md` (line 161).

## Read first: the original ticket of this task

The original ticket of the old run is in your worktree: `ai_docs/plans/fleetd-build/tickets/61-wrapper-facts-go-by-attempt-files.md`. Read it before the steps below. It is the source of truth for the intent, the fixes (for example "fix A") and the tests. When it and this ticket do not agree on the product behavior, the original ticket wins; send a NOTE that names the difference. The rules of this ticket about the sandbox, the Check and the merge still apply. The notes of the old run are in `ai_docs/plans/fleetd-build/notes/` (find your task with `ls ai_docs/plans/fleetd-build/notes | grep -i <T-number>`).

## Goal

The worker wrapper (`horch worker`) sends no request of class `operator` and never reads `<fleet root>/run/operator.token`. Its facts go through the attempt files (`crates/horch-core/src/fleetd/task_execution/domain/task_files.rs` and the attempt wrapper/outbox files), and legacy pane launch gives the wrapper no path or right to read the operator token.

## Requirements

- **L5 (T-899)**:
  1. Remove any operator-class request sent from the worker wrapper path (including `ExecutionStore` calls in `crates/horch-core/src/execution/store.rs:1034` that call `channel_client::send(..., Kind::SessionRecords, ...)` from a worker wrapper, or route wrapper session/record facts through the attempt files).
  2. In `crates/horch-core/src/execution/service.rs:278` (`run`), stop passing operator-token access to `horch worker`, and update the doc comment and launch environment so that legacy pane launch gives the wrapper no path to read `<fleet root>/run/operator.token`.
  3. In `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs:58`, refuse to read `operator.token` or send an `operator` request when the caller is inside a worker wrapper or fleet attempt.
  4. Prove with tests:
     - the wrapper process cannot read `<fleet root>/run/operator.token`;
     - a wrapper request of class `operator` is refused and recorded;
     - the legacy pane launch gives the wrapper no path to the token.

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

1. Inspect `crates/horch-core/src/execution/store.rs:1034`, `crates/horch-core/src/execution/lifecycle.rs`, `crates/horch-core/src/execution/service.rs:278`, and `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs:58`. Identify every place where `horch worker` (the wrapper) calls `ExecutionStore` or `channel_client::send` or receives `FLEET_ROOT`.
   - Check: write down the 3 tests in `crates/horch-core/tests/p6_authorize.rs` and `crates/horch-e2e/tests/p5_operator_boundary.rs` first and see them fail.
2. Update `crates/horch-core/src/execution/lifecycle.rs` and `crates/horch-core/src/execution/store.rs` so that the worker wrapper records its session and lifecycle facts only through attempt files (`task_files.rs` / wrapper files) and never calls `channel_client::send` with an `Operator` kind or reads `<fleet root>/run/operator.token`.
   - Check: `scripts/dev-sandbox.sh cargo test -p horch-core --lib execution`
3. Update `crates/horch-core/src/execution/service.rs:278` and `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs:58` so that legacy pane launch gives `horch worker` no path to the operator token and `channel_client` never reads `operator.token` for a wrapper process. Ensure a wrapper request of class `operator` on the channel is refused by `authorize` (`fleet_process`) and recorded as a refused request row.
   - Check: `scripts/dev-sandbox.sh cargo test -p horch-core --test p6_authorize --test p7_operator_boundary`
4. Build the workspace binaries and run the e2e operator boundary suite:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p5_operator_boundary`
   - Check: all tests in `p5_operator_boundary` pass.
5. Run the full ticket Check.

## Out of scope

- Touching `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` or any file owned by C3 through C10 where `Own` overlaps.

## Done when

The wrapper sends no operator-class request and never reads `operator.token`, the 3 required tests pass, and the Check passes.
