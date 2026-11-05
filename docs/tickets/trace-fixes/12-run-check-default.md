# 12: Run default check, no silent no-check, shared diff base

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- finish taskledger trf_16 trf_17 trf_18 && cargo test -p horch -- taskledger trf_16`
**Own:** `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch/src/cmd/taskledger.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 03 (One schema migration for every new field); 06 (Land: reset after a red check, recover a timed-out lock). Wave 2.

## Goal

Stop work landing untested (G3) and give shared tasks real review input (G5) (D5).

## Requirements

- **TRF-16**: `run new --check` sets the run default check, and a task with no `Check` inherits it. Test name starts with `trf_16_`.
- **TRF-17**: `land` refuses a task with no check unless its ticket says `**Check:** none`. Test name starts with `trf_17_`.
- **TRF-18**: A shared-checkout task diffs from its claim commit, so code-sweep and complexity-sweep apply. Test name starts with `trf_18_`.

## Steps

1. `run new --check` stores `default_check`; a task with no Check uses it.
2. `check_command` (land.rs) returns the task's check, else the run's; with neither and no `check_none`, check fails with a message that names `**Check:** none`.
3. For a shared-checkout task, compute the diff base from the commit at claim time, not `run_branch...HEAD`.

## Done when

The 3 tests pass.

## Report

Write `ai_docs/reports/trace-fixes/12.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
