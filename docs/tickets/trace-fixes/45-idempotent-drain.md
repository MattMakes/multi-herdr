# 45: An outbox drain never makes duplicate mail

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- outbox mail trf_03 trf_21 && cargo test -p horch -- wake trf_21`
**Own:** `crates/horch-core/src/taskledger/outbox.rs`, `crates/horch-core/src/messaging/mail.rs`, `crates/horch/src/cmd/taskledger_wake.rs`

Spec: `docs/specs/trace-fixes.md` (C2, C3, C7, D7, R10; conventions in section 8). Added by the orchestrator after review 31.
Review: `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/31.md`. Read your findings there first; they give file:line, a reproduction and a correction.
Waits for: nothing.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Goal

Fix review finding I-01: a crash between the durable enqueue and `mark_delivered` makes a 2nd mail with a new id on replay.

## Requirements

- **TRF-21** (extends): replaying an outbox row after a crash reuses the first durable mail; the mailbox holds exactly 1 pending file for 1 row. Test name starts with `trf_21_`.

## Steps

1. Derive the mail id from the outbox row id (stable, deterministic), or pass the row id as an idempotency key to the enqueue in `mail.rs`.
2. The enqueue finds an existing pending or acknowledged mail with that id and returns it instead of writing a new file.
3. Keep the C2 line format: the 8-hex short id still comes from the mail id.
4. Test `trf_21_replayed_row_reuses_its_mail`: enqueue, skip `mark_delivered`, drain again, assert 1 file.

## Done when

The check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/45.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
