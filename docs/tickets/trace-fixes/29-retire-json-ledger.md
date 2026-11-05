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

## Done when

TRF-45 passes; no code writes a session-ledger JSON file.

## Report

Write `ai_docs/reports/trace-fixes/29.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
