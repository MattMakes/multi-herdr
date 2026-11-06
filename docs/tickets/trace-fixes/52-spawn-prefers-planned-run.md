# 52: A plain spawn prefers the planned run over the steward's solo run

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch -- trf_34 trf_43 spawn recipes && cargo test -p horch-e2e --test taskledger`
**Own:** `crates/horch/src/cmd/taskledger_spawn.rs`, `crates/horch/src/cmd/recipes.rs`, `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/main.rs`, `crates/horch/src/cmd/cost.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after report 49.
Prior work: tickets 20, 43, 49; report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/49.md`, section "Follow-ups outside scope".
Waits for: 49.

## Goal

Since ticket 49, a fleet start with parked worktrees queues the worktree-steward work with `horch ledger task`, which makes a solo run in the workspace. When the orchestrator then starts its planned run with `horch ledger run new`, the workspace has 2 active runs, and every plain `horch spawn <persona>` fails in `run_to_join` (`crates/horch/src/cmd/taskledger_spawn.rs`): "2 active runs in workspace ...". The core resolver already has the rule: `planned_first` in `crates/horch-core/src/taskledger/runs.rs` drops solo runs when a planned run is among them.

## Requirements

- **TRF-34** (continued): a task-less spawn with no `--run` in a workspace whose active runs are 1 planned run plus solo runs joins the planned run. Two planned runs still refuse. Test names start with `trf_34_`.

## Steps

1. In `run_to_join`, apply the same rule as `planned_first` before the match (make `planned_first` `pub` in `runs.rs` and reuse it, or move it; do not copy it). `runs.rs` is approved outside Own for that 1 change.
2. The steward spawn in `recipes.rs` names its solo run with `--run <slug>`, so it never depends on the rule.
3. `recipes.rs` fleet start: do not print the `Run next: horch spawn worktree-steward --run <slug>` line from `taskledger::run` when the fleet spawns the steward itself (report 49, item 2).
4. `SpawnArgs::task` and the `task` argument of `claims_a_task` in `spawn.rs` are always empty now. Delete them and update `main.rs` and `cost.rs`.
5. Tests: `trf_34_a_plain_spawn_joins_the_planned_run_beside_a_solo_run`, `trf_34_two_planned_runs_still_refuse`. Run the Check.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Done when

The Check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/52.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
