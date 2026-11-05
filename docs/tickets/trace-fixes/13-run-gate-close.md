# 13: ledger run gate, and close guards

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger trf_19 trf_20 && cargo test -p horch -- taskledger trf_19 trf_20`
**Own:** `crates/horch-core/src/taskledger/runs.rs`, `crates/horch-core/src/taskledger/finish/pr.rs`, `crates/horch/src/cmd/taskledger.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 11 (One 'run settled' predicate). Wave 3.

## Goal

Move the end of a run from the prompt into code (C5, D4).

## Requirements

- **TRF-19**: `ledger run gate` runs the run check on the run-branch tip and records tip, command, exit code and log. Test name starts with `trf_19_`.
- **TRF-20**: `run close` refuses without a passing gate on the current tip, or while the PR is neither merged nor declined. Test name starts with `trf_20_`.

## Steps

1. Add `horch ledger run gate [--check <cmd>]` writing a `run_gates` row.
2. `run close` uses the C6 predicate, then requires a passing gate for the current tip and the PR merged (query with gh in pr.rs) or `--pr-declined "<reason>"`.
3. With no remote or no gh, the PR condition is not applicable and close says so.

## Done when

Both tests pass with a fake gh.

## Report

Write `ai_docs/reports/trace-fixes/13.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
