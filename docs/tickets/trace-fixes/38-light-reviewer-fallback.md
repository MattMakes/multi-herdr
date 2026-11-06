# 38: A light reviewer falls back to the other vendor's light reviewer

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- codesweep trf_27 trf_30`
**Own:** `crates/horch-core/src/taskledger/finish/codesweep.rs`, `crates/horch-core/tests/trf_27_review_loop.rs`

Spec: `docs/specs/trace-fixes.md` (D9, D12; conventions in section 8). Added by the orchestrator after report 18.
Waits for: nothing.

## Goal

`other_reviewer` in `crates/horch-core/src/taskledger/finish/codesweep.rs` (near line 351) knows only the 2 full
personas. `architect-reviewer-light` falls back to `architect-reviewer` (same vendor, full cost), and
`codex-reviewer-light` falls back to `architect-reviewer` (full cost). A fallback must switch vendor and keep the
round's cost tier.

## Requirements

- **TRF-30**: rounds 2-3 use the cheaper reviewer persona, including after a fallback. Test name starts with `trf_30_`.

## Steps

1. Map: `architect-reviewer` <-> `codex-reviewer`, and `architect-reviewer-light` <-> `codex-reviewer-light`. An unknown persona keeps today's default.
2. Check every caller of `other_reviewer` (for example `requeue_cancelled`) still gets the right tier.
3. Write `trf_30_light_reviewer_falls_back_to_the_other_light_reviewer`, a table test over all 4 personas.

## Done when

The check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/38.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
