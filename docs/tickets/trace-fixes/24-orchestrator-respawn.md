# 24: Respawn a dead orchestrator, rarely

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- watchdog orchestrator trf_38 trf_39 trf_40 && cargo test -p horch -- watch recipes trf_38 trf_39 trf_40`
**Own:** `crates/horch-core/src/taskledger/watchdog.rs`, `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch/src/cmd/recipes.rs`, `crates/horch-core/src/taskledger/orchestrator.rs`, `crates/horch-core/src/taskledger/mod.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 15 (Watchdog: 30 s reconcile and re-wake); 23 (horch ledger handoff). Wave 7.

## Goal

Implement contract C7 (D6).

## Requirements

- **TRF-38**: When the orchestrator's agent PID is gone in an active run, horch respawns it with the handoff, decisions and status. Test name starts with `trf_38_`.
- **TRF-39**: No respawn after a deliberate stop or a closed run. Test name starts with `trf_39_`.
- **TRF-40**: One orchestrator per workspace; at most 1 respawn per 10 min and 3 per run; each respawn is an event shown by status. Test name starts with `trf_40_`.

## Steps

1. The watchdog checks the registered orchestrator's agent PID each pass.
2. On a confirmed death in an active run with no deliberate stop, and inside the rate limits, respawn through `fleet_with` with the first instruction of C7.
3. Record `orchestrator_stopped_at` on `horch fleet stop` and `run close`.
4. Hold a per-workspace orchestrator lock so 2 can never run.

## Done when

The 3 tests pass with a fake herdr and a fake clock.

## Report

Write `ai_docs/reports/trace-fixes/24.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
