# 42: An orchestrator launch claims its run: workspace recorded, stop cleared

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- orchestrator trf_38 trf_39 trf_40 && cargo test -p horch -- recipes trf_38 trf_39 trf_40`
**Own:** `crates/horch-core/src/taskledger/orchestrator.rs`, `crates/horch-core/src/taskledger/runs.rs`, `crates/horch/src/cmd/recipes.rs`

Spec: `docs/specs/trace-fixes.md` (C7, D6, D13; conventions in section 8). Added by the orchestrator after report 24.
Prior work: `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/24.md`.
Waits for: nothing.

## Goal

Ticket 24 left 2 gaps (report 24, "Gaps"):
1. `runs.orchestrator_stopped_at` is never cleared. After `horch fleet stop`, a new fleet start for the same run keeps respawn off for the rest of the run, with no message.
2. An orchestrator that the operator starts in a new workspace does not update `runs.workspace_id`. Respawn then sees the old workspace as gone and skips, and a task-less spawn in the new workspace does not join the run (D13, ticket 20).

## Requirements

- **TRF-38** (extends): when an orchestrator takes the pane-launch lock for an active run, horch records the pane's workspace on the run and clears `orchestrator_stopped_at`, in 1 transaction. Test name starts with `trf_38_`.

## Steps

1. Read `orchestrator.rs` and the pane-launch lock in `recipes.rs` (ticket 24).
2. In the same transaction that registers the orchestrator for the run, set `runs.workspace_id` to the pane's workspace and set `orchestrator_stopped_at` to NULL. Write 1 `note` event when either value changes, naming the old and new values.
3. Never move a run to a workspace that already has another active non-solo run. Refuse with 1 line that names both runs.
4. Tests:
   - `trf_38_launch_after_fleet_stop_clears_the_stop`;
   - `trf_38_launch_in_a_new_workspace_moves_the_run`;
   - `trf_38_launch_refuses_a_workspace_with_another_active_run`.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

Out of scope: docs (ticket 30).

## Done when

The check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/42.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
