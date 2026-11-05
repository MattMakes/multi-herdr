# 23: horch ledger handoff

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger trf_37 && cargo test -p horch -- taskledger trf_37`
**Own:** `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/workers.rs`, `crates/horch/src/cmd/taskledger.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 17 (Answers close 1 question by id); 22 (cancel and abandon stop the worker). Wave 6.

## Goal

One handoff path for compaction and respawn (C4, D15).

## Requirements

- **TRF-37**: `ledger handoff` ends the worker handed off, requeues the task first holding its worktree, and the next claim prints the handoff path. Test name starts with `trf_37_`.

## Steps

1. Implement contract C4 in 1 transaction.
2. The next claim prints `PRIOR WORK: read <path> first` before the ticket snapshot.

## Done when

TRF-37 passes.

## Report

Write `ai_docs/reports/trace-fixes/23.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
