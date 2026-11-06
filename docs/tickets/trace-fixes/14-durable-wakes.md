# 14: Wakes through a transactional outbox

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger outbox trf_21 && cargo test -p horch -- wake trf_21`
**Own:** `crates/horch/src/cmd/taskledger_wake.rs`, `crates/horch-core/src/taskledger/outbox.rs`, `crates/horch-core/src/taskledger/mod.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 03 (One schema migration for every new field); 05 (Mail ids on every line, and a courier that stays while mail is pending); 11 (One 'run settled' predicate). Wave 3.

## Goal

Make every wake durable (C3, D7).

## Requirements

- **TRF-21**: A wake is written to the outbox in the same transaction as its ledger change and reaches durable mail even after a failed send. Test name starts with `trf_21_`.

## Steps

1. Write the outbox row inside the transaction that changes the ledger. Do not send inside the transaction.
2. A drain function moves undelivered rows into durable mail (ticket 05's format) and sets `delivered_at`. Call it after each ledger command and from the courier.
3. Replace every direct wake send with an outbox write. A failed send leaves the row for the next drain.

Amendment (report 11): the "run settled" wake from `settled_note` in `crates/horch/src/cmd/taskledger_wake.rs` must be 1 outbox row per settle, keyed so a repeat of the same settled state writes no new row.

## Done when

TRF-21 passes, including a test where the first send fails.

## Report

Write `ai_docs/reports/trace-fixes/14.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
