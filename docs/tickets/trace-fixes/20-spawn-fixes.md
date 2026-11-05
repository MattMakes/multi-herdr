# 20: Spawn: resume claims, and runs stay in their workspace

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger trf_33 trf_34 && cargo test -p horch -- spawn trf_33 trf_34`
**Own:** `crates/horch/src/cmd/taskledger_spawn.rs`, `crates/horch/src/cmd/spawn.rs`, `crates/horch-core/src/taskledger/runs.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 02 (Remove competitive dataset rounds); 13 (ledger run gate, and close guards); 16 (horch spawns code-sweep reviewers). Wave 5.

## Goal

Fix G11 and G12 (D13, D16).

## Requirements

- **TRF-33**: `spawn --resume` inside a run gets the claim instruction. Test name starts with `trf_33_`.
- **TRF-34**: A task-less spawn joins only its own workspace's run. Test name starts with `trf_34_`.

## Steps

1. A `--resume` spawn that joins a run gets `CLAIM_TASK` as its task.
2. `run new` fills the existing `runs.workspace_id` when it runs in a pane (today only a later ledger command from the orchestrator pane sets it). A task-less spawn joins only its own workspace's active run; `--run <slug>` reaches another one.

## Done when

Both tests pass.

## Report

Write `ai_docs/reports/trace-fixes/20.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
