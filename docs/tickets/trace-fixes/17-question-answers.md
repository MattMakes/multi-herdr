# 17: Answers close 1 question by id

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- messaging taskledger trf_25 trf_26 && cargo test -p horch -- messaging trf_25 trf_26`
**Own:** `crates/horch/src/cmd/messaging.rs`, `crates/horch-core/src/taskledger/workers.rs`, `crates/horch-core/src/messaging/mail.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 02 (Remove competitive dataset rounds); 05 (Mail ids on every line, and a courier that stays while mail is pending). Wave 2.

## Goal

Close questions precisely (C2, D8).

## Requirements

- **TRF-25**: `horch tell <role> --answers <id>` closes only that question; a tell without it closes none. Test name starts with `trf_25_`.
- **TRF-26**: A horch-raised question closes when the ledger action it names happens. Test name starts with `trf_26_`.

## Steps

1. Add `horch tell <role> --answers <id>`; it renders `[orchestrator re #<id>]` and closes that question only. A plain tell closes none.
2. Keep question waits in `workers.rs` keyed by question id, not by role.
3. A horch-raised QUESTION names its ledger action and closes when that action runs; otherwise send it as NOTE.

## Done when

Both tests pass.

## Report

Write `ai_docs/reports/trace-fixes/17.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
