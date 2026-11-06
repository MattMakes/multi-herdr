# 27: Retire horch assign and task-text spawns

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core --lib && cargo test -p horch -- trf_43`
**Own:** `crates/horch/src/cmd/messaging.rs`, `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/main.rs`, `crates/horch-core/src/execution/plan.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 17 (Answers close 1 question by id); 25 (Record spawn-gate decisions); 26 (horch ledger task: a cheap one-off). Wave 7.

## Goal

Remove the push dispatch model (D1, U1).

## Requirements

- **TRF-43**: `horch assign` is gone and `horch spawn` with task text refuses, printing the `horch ledger task` command. Test name starts with `trf_43_`.

## Steps

1. Remove `horch assign`.
2. `horch spawn <persona> "<task>"` refuses and prints the matching `horch ledger task` command. `--resume` keeps working through the ledger.
3. Update CLI tests that used assign or task-text spawns.

Amendment (report 26), do this before you retire assign: `horch ledger task` (ticket 26, `one_off_task` in `crates/horch/src/cmd/taskledger.rs`) is the replacement for `horch assign`, but a one-off with no `--file` and no `--check` can never land (D5), because it has no ticket line `**Check:** none`. Make `--check none` set the task's `check_none`, and make the no-check warning name both `--check "<cmd>"` and `--check none`. For a one-off, the claim text must not say the snapshot is "as add-map queued it". `crates/horch/src/cmd/taskledger.rs` is approved outside Own. Add 1 test whose name starts with `trf_42_`: a one-off with `--check none` lands.

## Done when

TRF-43 passes; no code path types a task into a pane.

## Report

Write `ai_docs/reports/trace-fixes/27.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
