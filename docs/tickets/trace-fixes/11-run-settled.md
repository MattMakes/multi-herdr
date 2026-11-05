# 11: One 'run settled' predicate

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger trf_15 && cargo test -p horch -- taskledger trf_15`
**Own:** `crates/horch-core/src/taskledger/status.rs`, `crates/horch-core/src/taskledger/runs.rs`, `crates/horch/src/cmd/taskledger_wake.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 03 (One schema migration for every new field). Wave 2.

## Goal

Give 'done' one meaning (C6, D3).

## Requirements

- **TRF-15**: One function decides "run settled"; the wake, `status` exit code and `run close` agree on a fixture of every worktree state. Test name starts with `trf_15_`.

## Steps

1. Write 1 function for C6 in `status.rs`. Replace the 3 current definitions (`taskledger_wake.rs:80-95`, `status.rs:338-346`, the open-items part of `runs.rs`).
2. `ledger status` exits 0 only when settled and lists each unsettled item.
3. Test with a fixture that has 1 worktree in every state.

## Done when

TRF-15 passes; the 3 callers use the 1 function.

## Report

Write `ai_docs/reports/trace-fixes/11.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
