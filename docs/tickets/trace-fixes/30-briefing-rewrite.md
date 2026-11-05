# 30: Rewrite the briefings and docs for the new behaviour

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- golden trf_46`
**Own:** `teammates/_base/`, `skills/orchestrate/`, `docs/fleet-workflow.md`, `docs/command-flow.md`, `docs/specs/task-ledger.md`, `docs/specs/context-policy.md`, `README.md`, `CHANGELOG.md`, `crates/horch-core/tests/golden/`, `crates/horch-core/tests/trf_46_briefings.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 07 (complexity-sweep covers Rust); 18 (Bounded, incremental, cheaper code review); 29 (Stop writing the JSON session ledger). Wave 10.

## Goal

Make the prompts describe what the code now does, in 1 pass (U1, U3, U4).

## Requirements

- **TRF-46**: The briefings contain no push dispatch, and every `context --over` state has an instruction. Test name starts with `trf_46_`.

## Steps

1. Orchestrator: pull only (no 'Read and follow <path>' spawns, no assign), idea → spec → tickets → `add-map --dry-run` → `add-map`, `run gate` and close, `--answers`, reviewers spawned by horch, the respawn first instruction.
2. Worker: `ledger progress` only, 'ticket' not 'plan file', the handoff route, an instruction for every state id from ticket 08's report.
3. Docs and specs: the new commands, contracts C1-C7, timings. README and CHANGELOG: the removal from ticket 02's report, and 1 user-visible CHANGELOG line for the run.
4. Re-bless only the goldens these edits change. Write `trf_46_*`: no push phrase in the briefings; every state id has an instruction.

## Done when

TRF-46 and the golden tests pass.

## Report

Write `ai_docs/reports/trace-fixes/30.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
