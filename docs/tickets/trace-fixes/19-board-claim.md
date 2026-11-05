# 19: Board and claim dead spots

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger trf_31 trf_32`
**Own:** `crates/horch-core/src/taskledger/tasks.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 10 (add-map reads the contract, dry-runs, snapshots and refuses duplicates). Wave 3.

## Goal

Fix D1 and D2 (D16).

## Requirements

- **TRF-31**: The board reserves a slot only for tasks whose persona has a live or starting worker. Test name starts with `trf_31_`.
- **TRF-32**: A worker whose claim finds no task ends and the orchestrator is told. Test name starts with `trf_32_`.

## Steps

1. In `Board::plan`, reserve a startable task's slot only when its persona has a live or starting worker.
2. A claim that finds no task ends the worker (`no_task` with `ended_at`) and writes a `no-task` outbox row. If closing the pane needs a file outside Own, send QUESTION.
3. Gap G-A (report 01, item 3): in an active run, `horch spawn <persona>` with no task refuses when the live and starting workers of that persona already cover its `plan-workers` demand. The message names `horch ledger plan-workers`. Approved outside Own: `crates/horch/src/cmd/spawn.rs`, only for this guard. If T-2 has not landed, rebase on the run branch before land. Test name starts with `trf_31_`.

## Done when

Both tests pass.

## Report

Write `ai_docs/reports/trace-fixes/19.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
