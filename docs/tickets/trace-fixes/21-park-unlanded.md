# 21: Park unlanded work instead of cancelling it

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger triage trf_35`
**Own:** `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/taskledger/triage/`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 12 (Run default check, no silent no-check, shared diff base); 19 (Board and claim dead spots). Wave 4.

## Goal

Keep unlanded work in the queue (G13, D16).

## Requirements

- **TRF-35**: `done --unlanded` parks the task with its worktree; dependents stay blocked on it; `requeue` resumes on the same worktree. Test name starts with `trf_35_`.

## Steps

1. `done --unlanded` sets the task queued with `held_reason = unlanded` and keeps the worktree attached.
2. `requeue` of a parked task resumes it on the same worktree. Dependents stay blocked on it.
3. Triage FINISH reuses the parked task instead of queuing a new one.

## Done when

TRF-35 passes.

## Report

Write `ai_docs/reports/trace-fixes/21.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
