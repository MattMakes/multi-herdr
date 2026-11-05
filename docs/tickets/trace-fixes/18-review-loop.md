# 18: Bounded, incremental, cheaper code review

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- codesweep trf_27 trf_28 trf_29 trf_30`
**Own:** `crates/horch-core/src/taskledger/finish/codesweep.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 03 (One schema migration for every new field); 10 (add-map reads the contract, dry-runs, snapshots and refuses duplicates). Wave 3.

## Goal

End the review loop (L1, U6) and cut review cost (D9, D12).

## Requirements

- **TRF-27**: Code-sweep runs at most 3 rounds; after round 3 an unresolved Important finding becomes a QUESTION. Test name starts with `trf_27_`.
- **TRF-28**: Rounds 2 and 3 review only the diff since the last reviewed tree. Test name starts with `trf_28_`.
- **TRF-29**: Finding ids are stable across rounds; answers and waivers carry; reviewers see `answers.md` and accept or reject each answer. Test name starts with `trf_29_`.
- **TRF-30**: A round has at most 3 reviewers; `Review` none, light and full are honoured; rounds 2-3 use the cheaper reviewer model. Test name starts with `trf_30_`.

## Steps

1. Cap rounds at 3. After round 3, an unresolved Important finding writes a QUESTION outbox row instead of a round 4.
2. Rounds 2 and 3 diff from the last reviewed tree, not `run_branch...HEAD`.
3. Finding id = hash of file, rule and normalised text. Answers and waivers carry by id. The review ticket text names `answers.md`; reviewers mark each answer accepted or rejected.
4. Honour the task's `review_mode`: none = not applicable, light = 1 round and 2 reviewers, full = up to 3 rounds. At most 3 reviewers per round. Rounds 2-3 use the cheaper reviewer persona or model.

## Done when

The 4 tests pass.

## Report

Write `ai_docs/reports/trace-fixes/18.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
