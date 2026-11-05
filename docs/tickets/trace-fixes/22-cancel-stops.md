# 22: cancel and abandon stop the worker

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger stop trf_36 && cargo test -p horch -- taskledger trf_36`
**Own:** `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/stop.rs`, `crates/horch/src/cmd/taskledger.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 21 (Park unlanded work instead of cancelling it). Wave 5.

## Goal

Fix G14 (D16).

## Requirements

- **TRF-36**: `cancel` and `run close --abandon` signal the recorded PIDs of the task's worker. Test name starts with `trf_36_`.

## Steps

1. `cancel` and `run close --abandon` stop the task's worker by its recorded PIDs, through the same path as `ledger stop`.
2. A cancel no longer writes a requeue first.

## Done when

TRF-36 passes with fake processes.

## Report

Write `ai_docs/reports/trace-fixes/22.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
