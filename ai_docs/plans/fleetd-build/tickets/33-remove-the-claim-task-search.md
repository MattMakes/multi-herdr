# Remove the task search of horch ledger claim (FR-5): fixtures bind through 1 helper, and the interim claim paths are gone

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `cargo test --workspace --locked --no-fail-fast && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs`, `crates/horch-core/tests/`, `crates/horch-e2e/tests/`, `crates/horch/tests/`

## Goal

No code path lets a worker find and take a task by a search (`horch ledger claim`). fleetd binds
each task (FR-5). Each test fixture that needs a bound task uses 1 shared bind helper.

## Context

- T-352 kept the task search of `horch ledger claim`, because about 40 test files use it as a
  fixture and a red run branch stops each parallel worker. This ticket removes it. Other tasks
  run beside it: merge the run branch often, and move each new fixture that a merge brings.
- Ticket 27 of fleetd-p5 (T-476) refuses `claim` and `worktree new` to a worker. Read its done
  summary first (`horch ledger show T-476`). Do not repeat its work; remove what stays.
- T-368 added an interim edge `(TaskBranch, building, landed)` for the claim path only in
  `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs`. It has a comment "interim".
  Remove it with the claim path.
- Read the done summaries of T-352, T-353, T-368 and of ticket 32 of this folder.

## Steps

1. List each caller of the claim task search: product code, tests and fixtures. Put the count in
   a progress note.
2. Write 1 bind helper for tests (in the shared test support of each crate that needs it). It
   binds a task to a worker through the same door that fleetd uses. Check: 1 test of the helper.
3. Move each fixture to the helper. Work crate by crate, and run the tests of the crate after
   each move.
4. Remove the task search of `horch ledger claim`. The command gives a clear refusal text that
   names the push flow, or it is removed, as the design and T-476 say. Check: 1 test.
5. Remove the interim edge of T-368 and its claim-only code. Check: the test of T-368 that the
   push path cannot use the edge is replaced by a test that the edge does not exist.
6. Run the Check. Each red test is yours to fix. Send `horch ledger progress` each 5 min in long
   runs.

## Constraints

- Simplified Technical English. No em dash.
- 1 door: each state change goes through `transition()`.
- Do not make an assert weaker, and do not add a test to an expected-failures list.
- No new schema step. If you think that you need one, send QUESTION and end your turn.
- Each test uses temporary folders. Never use the live ledger. Never install a development build.

## Done when

- `grep` finds no task search behind `horch ledger claim`, and no interim edge of T-368.
- The Check passes with no red test.
- The done summary names the helper, the count of moved fixtures, and each product file changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
