# 38: Fix R1-F4: isolate each feature and task branch in its own savepoint (OR-2)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test feature_states && scripts/dev-sandbox.sh cargo test -p horch-core --test feature_states_p6 && scripts/dev-sandbox.sh cargo test -p horch-core --test landing_states && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/integration/application/features.rs`, `crates/horch-core/src/fleetd/integration/application/landing.rs`, `crates/horch-core/tests/feature_states.rs`, `crates/horch-core/tests/landing_states.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p0-pp-p1.md` (finding R1-F4) and `docs/specs/fleetd/design.md` (rule OR-2 and design rule "one bad row fails only its row").

## Goal

`features::follow_in` and `landing::follow` isolate each feature row and each task branch row in its own SQLite savepoint, record one stable needs-you item (and event/cause) when a row fails, advance the pass cursor past the failed row, and continue processing the remaining rows in the same pass.

## Requirements

- **R1-F4 (Important — OR-2, design rule "one bad row fails only its row")**:
  - **Finding text and file:line**:
    `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4558` places all integration work in one savepoint and rolls all of it back on an error. `crates/horch-core/src/fleetd/integration/application/features.rs:108` (`follow_in` at `:112`) and `crates/horch-core/src/fleetd/integration/application/landing.rs:237` (`follow` at `:243`) propagate an error from one feature or branch without a per-row savepoint. A concrete case is one branch row whose referenced task or landing data cannot be read. Its `follow_one` error rolls back valid transitions for every other feature and branch selected in that pass. The same bad row is selected again, so unrelated landing state machines can hang indefinitely. Repair attempts already avoid this pattern with a per-branch savepoint at `crates/horch-core/src/fleetd/task_execution/application/repair.rs:323`.
  - **Design rules**:
    - **OR-2** (`docs/specs/fleetd/design.md:202`): "Every state change shall be an appended event with a cause, so any outcome can be explained after the fact."
    - **Design rule ("Three rules, rule 1: one bad row fails only its row")** (`docs/specs/fleetd/design.md`, and `crates/horch-core/src/fleetd/task_execution/application/repair.rs:330`): a single feature or branch that fails in a pass rolls back only its own savepoint, opens one stable needs-you item explaining the cause, advances the cursor, and lets the other rows in the pass commit.

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

1. Write the failing tests first:
   - In `crates/horch-core/tests/landing_states.rs`, add test function `invalid_branch_row_between_two_valid_rows_fails_only_its_row_and_advances_both_valid_rows`:
     - Create 3 non-terminal task branches in sorted order (for example `b-1`, `b-2`, `b-3`) where `b-1` and `b-3` are valid and ready to transition (for example their tasks are `verifying` or `done`), and `b-2` has unreadable/corrupt landing or task data that causes `follow_one` to return `Err` (for example a corrupted `landing_state` or missing referenced task row).
     - Call `landing::follow` in one transaction.
     - Assert that `landing::follow` succeeds (`Ok`), `b-1` and `b-3` both advance to their new states, the pass cursor advances past `b-2`, and one stable needs-you item is opened for `b-2` (and resolves once `b-2` is fixed or terminal).
   - In `crates/horch-core/tests/feature_states.rs`, add test function `invalid_feature_row_between_two_valid_rows_fails_only_its_row_and_advances_both_valid_rows`:
     - Create 3 live features in sorted order (for example `F-1`, `F-2`, `F-3`) where `F-1` and `F-3` are valid and ready to advance, and `F-2` causes `follow_one` to return an error (for example a failing git read or invalid state/data for `F-2`).
     - Call `features::follow` in one transaction.
     - Assert that `features::follow` succeeds (`Ok`), `F-1` and `F-3` both advance, and one stable needs-you item is opened for `F-2`.
   - Check: run both new tests through `scripts/dev-sandbox.sh` and see them fail because `follow_one` currently aborts the loop with `?`.
2. In `crates/horch-core/src/fleetd/integration/application/landing.rs`:
   - In `follow` (line 237), wrap `follow_one(l, tx, clock, name)` in `SAVEPOINT landing_branch` (following the pattern in `crates/horch-core/src/fleetd/task_execution/application/repair.rs:323-337`).
   - On `Ok(effect)`, `RELEASE landing_branch` and extend `effects`.
   - On `Err(e)`, `ROLLBACK TO landing_branch; RELEASE landing_branch`, open one stable needs-you item (for example kind `"landing_follow_failed"`, key `format!("landing_follow_failed:{name}")`, `Entity::TaskBranch`, `entity_id: name`) via `l.jobs.needs_you.open_item`, keep the cursor stepped past `name`, and continue the loop. Resolve the needs-you item when `follow_one` later succeeds or the branch becomes terminal.
3. In `crates/horch-core/src/fleetd/integration/application/features.rs`:
   - In `follow_in` (line 101), wrap `follow_one(f, tx, &id, clock)` in `SAVEPOINT feature_follow`.
   - On `Ok(step)`, `RELEASE feature_follow`, resolve any open failure needs-you item for `id`, and push `(id, step)`.
   - On `Err(e)`, `ROLLBACK TO feature_follow; RELEASE feature_follow`, open one stable needs-you item (for example kind `"feature_follow_failed"`, key `format!("feature_follow_failed:{id}")`, `Entity::Feature`, `entity_id: &id`) via `f.jobs.needs_you.open_item`, push `(id, Step::Waits(format!("{e:#}")))`, and continue the loop.
4. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` (kept untouched so this ticket does not conflict with the landing chain).

## Done when

`invalid_branch_row_between_two_valid_rows_fails_only_its_row_and_advances_both_valid_rows` and `invalid_feature_row_between_two_valid_rows_fails_only_its_row_and_advances_both_valid_rows` pass, a bad feature or branch row no longer rolls back other rows or stalls the cursor, and the ticket Check passes.
