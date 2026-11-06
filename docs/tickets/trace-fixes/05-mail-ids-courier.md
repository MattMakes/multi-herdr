# 05: Mail ids on every line, and a courier that stays while mail is pending

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- messaging trf_03 trf_04 && cargo test -p horch -- mail trf_03 trf_04`
**Own:** `crates/horch-core/src/messaging/mail.rs`, `crates/horch-core/src/messaging/message.rs`, `crates/horch-core/src/messaging/delivery.rs`, `crates/horch/src/cmd/mail.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: nothing; wave 1. Wave 1.

## Goal

Make every delivered line identifiable (C2) and stop the courier dropping pending mail.

## Requirements

- **TRF-03**: Every mail line carries its mail id, and a redelivery repeats the same id. Test name starts with `trf_03_`.
- **TRF-04**: The courier does not exit while pending mail exists. Test name starts with `trf_04_`.

## Steps

1. Render lines as `[<role> #<id>] ...` with the 8-hex mail id; keep the id on every redelivery.
2. Change the courier exit rule (`cmd/mail.rs` 10-min no-target exit): it exits only when no mail is pending.
3. Leave `horch tell --answers` and question closing to ticket 17.

## Done when

TRF-03 and TRF-04 tests pass; existing messaging tests pass or are updated with the new line format.

## Report

Write `ai_docs/reports/trace-fixes/05.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
