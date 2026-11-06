# 43: run new refuses a 2nd active run in a workspace

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- runs trf_34 && cargo test -p horch -- trf_34 run_new`
**Own:** `crates/horch-core/src/taskledger/runs.rs`, `crates/horch/src/cmd/taskledger.rs`

Spec: `docs/specs/trace-fixes.md` (D13; conventions in section 8). Added by the orchestrator after report 42.
Waits for: 27 (it edits `crates/horch/src/cmd/taskledger.rs`).

## Goal

D13: a run belongs to 1 workspace. Since ticket 20, a task-less spawn errors when its workspace has 2 or more
active runs, and since ticket 42 an orchestrator launch refuses such a workspace. But `horch ledger run new`
still creates a 2nd active non-solo run in a workspace that has one. The error then appears later, at a spawn,
far from its cause.

## Requirements

- **TRF-34** (extends): `horch ledger run new` refuses when the caller's workspace already has an active non-solo run. The message names that run and the 2 ways out: `horch ledger run close` it, or run `run new` in another workspace. Solo runs (`solo-<workspace>`) never count. A run with no workspace id never counts. Test name starts with `trf_34_`.

## Steps

1. Find the workspace lookup that `run new` uses (ticket 20: `sessionscmd::caller_workspace`).
2. Before the run is created, check for an active non-solo run with that `workspace_id`. Do the check in the same transaction as the insert.
3. Tests: `trf_34_run_new_refuses_a_2nd_active_run_in_a_workspace`; `trf_34_run_new_ignores_solo_and_workspaceless_runs`.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Done when

The check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/43.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
