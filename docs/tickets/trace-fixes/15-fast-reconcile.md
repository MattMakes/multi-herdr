# 15: Watchdog: 30 s reconcile and re-wake

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- watchdog trf_22 trf_23 && cargo test -p horch -- watch trf_22 trf_23`
**Own:** `crates/horch-core/src/taskledger/watchdog.rs`, `crates/horch/src/cmd/taskledger_watch.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 14 (Wakes through a transactional outbox). Wave 4.

## Goal

Recover from any missed wake within minutes (D7).

## Requirements

- **TRF-22**: A task startable for 2 min with no starting or claiming worker gets exactly 1 re-wake per board state. Test name starts with `trf_22_`.
- **TRF-23**: A spawned worker that has not claimed within 3 min is flagged once. Test name starts with `trf_23_`.

## Steps

1. Every 30 s: drain the outbox, then read the board.
2. A task startable for 2 min with no starting or claiming worker writes 1 `startable` outbox row keyed by board hash.
3. A spawned worker with no claim after 3 min writes 1 `unclaimed-spawn` row.
4. The 600 s worker stall check is unchanged.

## Done when

Both tests pass with a fake clock.

## Report

Write `ai_docs/reports/trace-fixes/15.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
