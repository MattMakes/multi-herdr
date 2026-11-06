# 29: Stop writing the JSON session ledger

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core --lib && cargo test -p horch -- trf_45`
**Own:** `crates/horch-core/src/execution/legacy.rs`, `crates/horch-core/src/execution/lifecycle.rs`, `crates/horch-core/src/execution/service.rs`, `crates/horch/src/cmd/messaging.rs`, `crates/horch-core/src/taskledger/import.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 28 (Every session reader reads the task ledger). Wave 9.

## Goal

Make the task ledger the single record (D10, step 2 of 2).

## Requirements

- **TRF-45**: No code writes the JSON session ledger; `ledger import` still reads old JSON once; `done` and `note` are single commands. Test name starts with `trf_45_`.

## Steps

1. Remove every JSON write from spawn, worker start/exit, note and done.
2. `horch done` and `horch note` become the ledger commands (`ledger done`, `ledger progress`); keep the short names as aliases.
3. `ledger import` still reads old JSON ledgers once.

Amendment (report 28), do this before step 1: ticket 28 overlaid the task ledger in `ExecutionStore` and `Collector::open_in`, but some readers still read only the JSON, and they go blank once no code writes it. Move each to the ledger-backed read first, and show in the report that each still returns rows with no JSON writes:
- `crates/horch-core/src/sweep.rs` (reads the JSON only);
- `collect::read_ledgers` (`crates/horch-core/src/telemetry/collect.rs:37`) and its callers `crates/horch/src/cmd/usagecmd.rs:167,407`, `crates/horch/src/cmd/telemetry/screen.rs:373`, `collect.rs:1614`. Prefer making `read_ledgers` itself apply the ticket 28 overlay, so callers change little;
- harness on orchestrator rows (report 28, D-c): `insert_run` in `crates/horch/src/cmd/taskledger.rs` and the orchestrator rows in `crates/horch/src/cmd/taskledger_watch.rs` write the harness, as `pane_launch` in `recipes.rs` now does.
These files are approved outside Own. T-47 (opus-hardening-12) may edit `taskledger_watch.rs`, `cmd/taskledger.rs` and `messaging.rs` at the same time; rebase on conflict.

## Done when

TRF-45 passes; no code writes a session-ledger JSON file.

## Report

Write `ai_docs/reports/trace-fixes/29.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
