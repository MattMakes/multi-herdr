# 25: Record spawn-gate decisions

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch -- spawn trf_41`
**Own:** `crates/horch/src/cmd/spawn.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 20 (Spawn: resume claims, and runs stay in their workspace). Wave 6.

## Goal

Make the spawn gate measurable without letting the record route anything (D18).

## Requirements

- **TRF-41**: Every spawn-gate decision (NOTE, SUBSTITUTED, REFUSED) is recorded as a ledger event and never changes routing. Test name starts with `trf_41_`.

## Steps

1. Write 1 ledger event per gate decision: teammate, outcome, pool, reason.
2. Nothing reads it to decide a spawn.

## Done when

TRF-41 passes.

## Report

Write `ai_docs/reports/trace-fixes/25.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
