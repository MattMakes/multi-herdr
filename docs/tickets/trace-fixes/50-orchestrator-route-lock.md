# 50: horch register cannot steal the orchestrator route, and a pending orchestrator does not show as stale

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- status trf_39 && cargo test -p horch -- register trf_39`
**Own:** `crates/horch/src/cmd/messaging.rs`, `crates/horch-core/src/taskledger/status.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after report 44.
Prior work: ticket 44, report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/44.md`, section "Follow-ups outside scope".
Waits for: 44, 46.

## Goal

Ticket 44 fixed review 31 finding I-02 for `pane_launch` only: `register_pane` in `crates/horch/src/cmd/recipes.rs` takes the workspace orchestrator lock before `Mailbox::register_role`, so a losing launch never writes the orchestrator route. Two gaps stay.

## Requirements

- **TRF-39** (continued): no code path writes the `orchestrator` route without the workspace orchestrator lock. Test names start with `trf_39_`.

## Steps

1. `register` in `crates/horch/src/cmd/messaging.rs` (line 1088, `horch register <role>`) writes any role with no lock. For role `orchestrator`, take the same lock that `register_pane` takes (reuse that code; do not copy it). When another live process holds the lock, refuse with 1 line that names the holder pid and changes nothing. Other roles behave as today.
2. `crates/horch-core/src/taskledger/status.rs` marks a pre-claim row stale through `workers::is_stale` (line 279). Ticket 44 keeps a replacement orchestrator row in status `spawning` until a watchdog pass sees the lock holder, so the board can print `STALE` for it after 120 s. For a row of kind `orchestrator` in status `spawning`, print `launch pending` instead of `STALE`.
3. Tests: `trf_39_register_refuses_the_orchestrator_role_while_another_holds_the_lock`, `trf_39_register_takes_the_orchestrator_role_when_the_lock_is_free`, `trf_39_a_pending_orchestrator_row_is_not_stale`.
4. Run the Check.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Done when

The Check passes, and report 50 shows the refusal line.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/50.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
