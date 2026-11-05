# 16: horch spawns code-sweep reviewers

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger trf_24 && cargo test -p horch -- taskledger trf_24`
**Own:** `crates/horch/src/cmd/taskledger_finish.rs`, `crates/horch/src/cmd/taskledger_spawn.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 02 (Remove competitive dataset rounds); 14 (Wakes through a transactional outbox). Wave 4.

## Goal

Take reviewer spawning off the orchestrator (D11).

## Requirements

- **TRF-24**: Code-sweep review tasks are spawned by horch through the spawn gate; the orchestrator gets a NOTE. Test name starts with `trf_24_`.

## Steps

1. When finish queues review tasks, spawn 1 worker per review task through the normal spawn path and gate.
2. On REFUSED, leave the task queued and write a NOTE outbox row; on SUBSTITUTED, proceed and note it.
3. The orchestrator gets 1 NOTE that names the spawned reviewers.

## Done when

TRF-24 passes with a fake spawn.

## Report

Write `ai_docs/reports/trace-fixes/16.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
