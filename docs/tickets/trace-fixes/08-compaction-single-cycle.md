# 08: Compaction: show every state, and 1 cycle before handoff

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- compaction trf_08 trf_09 && cargo test -p horch -- context compact trf_08 trf_09`
**Own:** `crates/horch/src/cmd/compact.rs`, `crates/horch/src/cmd/context.rs`, `crates/horch-core/src/compaction/`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: nothing; wave 1. Wave 1.

## Goal

Remove the compaction states with no instruction (D5) and the unbounded cycle (L3).

## Requirements

- **TRF-08**: `horch context --over` lists every non-ok state, and each state has an instruction in the briefing. Test name starts with `trf_08_`.
- **TRF-09**: A session still over its threshold after 1 compaction is sent to the handoff route, not a new cycle. Test name starts with `trf_09_`.

## Steps

1. `context --over` lists every state that is not ok, including pending, compacting, unknown, no-session, not-read and no-transcript. Give each state a stable instruction id.
2. Write the list of state ids with 1 line each to your report; ticket 30 writes the briefing text for them.
3. After 1 compaction in a cycle, a session still over threshold gets the handoff route (C4) instead of a new request.
4. TRF-08's briefing half is checked by ticket 30; your test checks the state list and ids.

## Done when

TRF-08 (state list) and TRF-09 tests pass.

## Report

Write `ai_docs/reports/trace-fixes/08.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
