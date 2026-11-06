# 47: An auto-spawned reviewer claims the review it was spawned for

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-hardening` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- tasks claim trf_24 && cargo test -p horch -- spawn trf_24`
**Own:** `crates/horch/src/cmd/taskledger_spawn.rs`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/workers.rs`

Spec: `docs/specs/trace-fixes.md` (C2, C3, C7, D7, R10; conventions in section 8). Added by the orchestrator after review 31.
Review: `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/31.md`. Read your findings there first; they give file:line, a reproduction and a correction.
Waits for: nothing.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Goal

Fix review finding I-08: `CliSpawn` runs `horch spawn <persona> --run <slug>`, and the claim takes the first
startable task of the persona, which can be a different task than the review the NOTE names.

## Requirements

- **TRF-24** (extends): an auto-spawned reviewer claims the review task it was spawned for; the NOTE names only what it will claim. Test name starts with `trf_24_`.

## Steps

1. Reserve the review for the spawned worker: record the target task on the worker row at spawn (reuse a column if one fits; else add one in the unreleased v7 migration and say so). The claim of a worker with a target takes that task first, if it is still startable; else it claims as today and the NOTE text says so.
2. A reservation expires when the worker ends, so no task stays reserved by a dead worker.
3. Test `trf_24_auto_spawned_reviewer_claims_its_review_not_an_older_task`, using the reproduction in review 31 I-08.

## Done when

The check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/47.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
