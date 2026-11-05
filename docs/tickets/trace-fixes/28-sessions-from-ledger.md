# 28: Every session reader reads the task ledger

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core --lib && cargo test -p horch -- trf_44`
**Own:** `crates/horch-core/src/execution/records.rs`, `crates/horch-core/src/execution/store.rs`, `crates/horch/src/cmd/sessionscmd.rs`, `crates/horch/src/cmd/cost.rs`, `crates/horch/src/cmd/context.rs`, `crates/horch/src/cmd/compact.rs`, `crates/horch/src/cmd/channel.rs`, `crates/horch/src/cmd/mode.rs`, `crates/horch/src/cmd/doctor.rs`, `crates/horch/src/cmd/smoke.rs`, `crates/horch/src/cmd/recipes.rs`, `crates/horch-core/src/telemetry/`, `crates/horch-core/src/sweep.rs`, `crates/horch-core/src/sweep/`, `crates/horch-core/src/harness/launch.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 08 (Compaction: show every state, and 1 cycle before handoff); 09 (doctor warns about stale ambient herdr skills); 24 (Respawn a dead orchestrator, rarely); 27 (Retire horch assign and task-text spawns). Wave 8.

## Goal

Switch every reader of the JSON session ledger to the task ledger (D10, step 1 of 2).

## Requirements

- **TRF-44**: `horch sessions` and every session reader read the task ledger. Test name starts with `trf_44_`.

## Steps

1. Pin today's `horch sessions` output in a test before changing it.
2. Serve `SessionLedger` reads from SQLite worker rows behind the same API, then switch each caller.
3. Writers stay as they are; ticket 29 removes them.

## Done when

TRF-44 passes; `horch sessions` shows ledger workers with their claimed task.

## Report

Write `ai_docs/reports/trace-fixes/28.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
