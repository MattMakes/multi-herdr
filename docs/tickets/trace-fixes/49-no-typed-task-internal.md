# 49: No internal code path types a task into a pane

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch -- trf_43 smoke recipes taskledger_spawn`
**Own:** `crates/horch/src/cmd/smoke.rs`, `crates/horch/src/cmd/recipes.rs`, `crates/horch/src/cmd/taskledger_spawn.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after report 27.
Prior work: ticket 27, report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/27.md`, section "Follow-ups outside scope".
Waits for: 27.

## Goal

Ticket 27 (TRF-43) removed task text from the `horch spawn` CLI. Internal callers still type a task into a pane, and dead code is left.

## Requirements

- **TRF-43** (continued): no code path in horch types a task into a pane. Test names start with `trf_43_`.

## Steps

1. `cmd/recipes.rs`, the worktree-steward spawn (`"parked worktrees: N"`): queue the work as a one-off ledger task (the same code that `horch ledger task --persona worktree-steward --check none` runs), then spawn the steward with no task text, so it claims that task.
2. `cmd/smoke.rs` (`spawn_smoke`): do the same, or, if smoke runs outside any ledger run on purpose, say why in the report and send the orchestrator 1 QUESTION before you change its behaviour.
3. `cmd/recipes.rs:506` prints `horch spawn <teammate> "task"`. Change it to the current form (`horch ledger task '<task>' --persona <teammate>`, then `horch spawn <teammate>`).
4. `cmd/taskledger_spawn.rs`: delete the "this spawn has task text, so it runs outside task-ledger run" NOTE path, which nothing can reach now. Delete what only it used.
5. Add 1 test per changed caller, names starting with `trf_43_`. Run the Check.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

Out of scope: `SessionLedger::assign_in` and the hidden `horch ledger legacy assign` (ticket 29 retires the JSON ledger). Prompt and doc text (ticket 30).

## Done when

The Check passes. The spawn requests built in `smoke.rs` (`spawn_smoke`, line 220) and `recipes.rs` (line 482) carry no task text (`task:` empty or the field removed), and the report says which.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/49.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
