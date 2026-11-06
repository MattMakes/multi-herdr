# 39: Round 2-3 review tasks queue with the other vendor's light reviewer as fallback

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- codesweep trf_27 trf_30`
**Own:** `crates/horch-core/src/taskledger/finish/codesweep.rs`, `crates/horch-core/tests/trf_27_review_loop.rs`

Spec: `docs/specs/trace-fixes.md` (D12; conventions in section 8). Added by the orchestrator after report 38.
Prior work: `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/38.md` (it names the 1-line change).
Waits for: nothing.

## Goal

When `reviewer_personas` in `crates/horch-core/src/taskledger/finish/codesweep.rs` first queues a round 2 or 3
review task, the fallback is the full persona of the same vendor. A fallback runs when the primary's pool cannot
serve, so a same-vendor fallback cannot help, and it is not the cheaper tier. Make the fallback the other vendor's
light reviewer, as `other_reviewer` does since ticket 38.

## Requirements

- **TRF-30**: rounds 2-3 use the cheaper reviewer persona, including the fallback. Test name starts with `trf_30_`.

## Steps

1. Apply the 1-line change that report 38 names: the round 2-3 fallback is `other_reviewer(<light primary>)`.
2. Update `trf_30_review_modes_reviewer_limits_and_the_cheaper_rounds` in `crates/horch-core/tests/trf_27_review_loop.rs` to assert the new pair.
3. Round 1 is unchanged.

## Done when

The check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/39.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
