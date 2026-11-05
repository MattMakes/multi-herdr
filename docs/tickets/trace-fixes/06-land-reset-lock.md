# 06: Land: reset after a red check, recover a timed-out lock

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- finish::land fsx trf_05 trf_06`
**Own:** `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch-core/src/fsx.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: nothing; wave 1. Wave 1.

## Goal

Stop a red land check from starting a new review round (L2) and stop a lock timeout from stranding a `landing` row (D7).

## Requirements

- **TRF-05**: A red check after a successful rebase in land resets the branch to its pre-rebase tip. Test name starts with `trf_05_`.
- **TRF-06**: A land-lock wait timeout resets the worktree row from `landing`; a lock is stale only after the max check timeout plus 10 min. Test name starts with `trf_06_`.

## Steps

1. After a successful rebase, a red check resets the branch to its pre-rebase tip before returning, as `docs/specs/task-ledger.md` already says.
2. On a lock-wait timeout, reset the worktree row from `landing` to open, like the Red path.
3. Set the stale-lock age to the task's max check timeout plus 10 min, and test the age before breaking a live holder's lock.

## Done when

TRF-05 and TRF-06 tests pass with a real git repo fixture.

## Report

Write `ai_docs/reports/trace-fixes/06.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
