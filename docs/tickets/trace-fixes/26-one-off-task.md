# 26: horch ledger task: a cheap one-off

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger trf_42 && cargo test -p horch -- taskledger trf_42`
**Own:** `crates/horch-core/src/taskledger/runs.rs`, `crates/horch/src/cmd/taskledger.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 20 (Spawn: resume claims, and runs stay in their workspace). Wave 6.

## Goal

Make pull-only dispatch cheap for single tasks (D1).

## Requirements

- **TRF-42**: `horch ledger task` queues a one-off task in the workspace's solo run and creates that run when needed. Test name starts with `trf_42_`.

## Steps

1. `horch ledger task "<title>" --persona <p> [--file <ticket>] [--check <cmd>]` queues 1 task in the workspace's solo run (`solo-<workspace>`), creating it with the minimal profile when absent.
2. Print the `horch spawn <persona>` line to run next.

## Done when

TRF-42 passes.

## Report

Write `ai_docs/reports/trace-fixes/26.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
