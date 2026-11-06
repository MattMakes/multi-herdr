# 44: Respawn survives a crash before launch, and a losing launch keeps the live route

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- orchestrator trf_38 trf_39 trf_40 && cargo test -p horch -- recipes trf_38 trf_39 trf_40`
**Own:** `crates/horch-core/src/taskledger/orchestrator.rs`, `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch/src/cmd/recipes.rs`, `crates/horch-core/src/messaging/mailbox.rs`

Spec: `docs/specs/trace-fixes.md` (C2, C3, C7, D7, R10; conventions in section 8). Added by the orchestrator after review 31.
Review: `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/31.md`. Read your findings there first; they give file:line, a reproduction and a correction.
Waits for: nothing.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Goal

Fix review findings C-01 and I-02.

## Requirements

- **TRF-38** (extends): a respawn whose launch did not happen (crash between commit and launch, or a failed launch) is retried on a later watchdog pass, inside the rate limits and the 10 min guard. Test names start with `trf_38_`.
- **TRF-39** (extends): only the holder of the workspace orchestrator lock can register the `orchestrator` mailbox route. Test names start with `trf_39_`.

## Steps

1. C-01: give the replacement orchestrator row a launch-pending state (reuse an existing column or status if one fits; the schema is v7 and unreleased, so a new column in the v7 migration is allowed if needed; say so in the report). Mark it live only after a pass observes the lock holder. Each pass reconciles a launch-pending row older than 1 pass: launch again, or count a failure. A failed launch does not end the budget; it counts 1 respawn.
2. I-02: in `pane_launch`, take the workspace orchestrator lock before role registration. A launch that fails the lock never touches `orchestrator.id`.
3. Tests: `trf_38_crash_before_launch_is_retried_on_the_next_pass`, `trf_38_failed_launch_keeps_the_remaining_budget`, `trf_39_losing_launch_keeps_the_live_route`.

## Done when

The check passes. The report maps C-01 and I-02 to the change and the test.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/44.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
