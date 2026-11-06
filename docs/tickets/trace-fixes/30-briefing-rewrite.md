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

Amendment (report 04): `teammates/_base/fleet-orchestrator.md` lines 160-173, its golden in `crates/horch-core/tests/golden_prompts.rs`, and `README.md` lines 430-436 still name `ai_docs/specs` and `ai_docs/tickets`. Change them to `docs/specs/<slug>.md` and `docs/tickets/<slug>/`. Bless only that golden.

Amendment (reports 06, 08):
- `docs/specs/context-policy.md` CTX-09: describe the new `--over` output and the instruction ids that report 08 lists.
- The briefing: add 1 instruction for each new context state id in report 08, including `handoff`.
- `docs/specs/task-ledger.md` section 5.5: name the land-lock stale age (check timeout plus 10 min) and the reset after a red land (report 06).

Amendment (report 05, incident of 2026-11-06):
- Describe the `[<role> #<id>]` mail tag in the INSTRUCTIONS text of `crates/horch-core/src/messaging/channel.rs` and in `teammates/_base/fleet-orchestrator.md`.
- Add 1 rule to the worker base briefing: never run a horch built from a worktree (`cargo run`, `target/*/horch`) without `HORCH_LEDGER_DB` set to a scratch file. Ticket 35 adds the code guard; the rule explains it.

Amendment (report 10): `horch ledger claim` now prints the ticket snapshot, then the path. Change the briefing line that tells a worker to read the ticket file in full: the snapshot is the ticket; read the file only for context.

Amendment (decision R11): add `question` to the C3 event list in `docs/specs/trace-fixes.md` section 3, and name the 2 light reviewer personas in the docs that list teammates.

Amendment (report 34): `docs/specs/architecture.md` lines near 1211 and 1274 still describe `footprint_bytes`, which ticket 34 deleted. The module doc of `crates/horch-core/src/harness/capabilities.rs` names the competition preflight. Fix both texts (approved outside Own).

## Done when

TRF-46 and the golden tests pass.

## Report

Write `ai_docs/reports/trace-fixes/30.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
