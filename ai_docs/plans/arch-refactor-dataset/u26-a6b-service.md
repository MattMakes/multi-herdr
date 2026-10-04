# U26 a6b-service: Execution, pure spawn plan, ExecutionService, worker lifecycle

Unit slug: `a6b-service`. Branch: `ard/a6b-service`. Phase: A6 (commits 2 to 4).
Requirements: ARC-15, ARC-16, ARC-18, ARC-22 (exit-code part), ARC-26, SKL-04 (wiring).

## GOAL

`horch spawn` is an application workflow built from a pure plan: a typed
`SpawnRequest` becomes an `ExecutionPlan` with no I/O, an `ExecutionService`
applies it in a fixed order with typed failures (`LaunchFailed`) and no
phantom live records, and `horch worker` runs a fixed startup order and
records the agent's exit. `cmd/spawn.rs` and `cmd/worker.rs` become thin.
All 5 harnesses pass an e2e lifecycle matrix (fresh, discovery, done, resume).

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §2 ("execution" bullets: `SpawnRequest`,
  `ExecutionPlan`, `plan_launch`, `finish_plan`, `SpawnError`), §3 "A6"
  commits 2, 3, 4 and the test list, §4 ARC-15, ARC-16, ARC-18, ARC-22,
  ARC-26 rows, §5 spawn fault points.
- Design doc `ai_docs/designs/2026-10-02-architecture-refactor-design.md`:
  §4.3 (`Execution` at about line 535, legacy compatibility), the
  execution planning block (`SpawnRequest` about 721, `ExecutionPlan` 755,
  `plan_launch` 777, `finish_plan` 779, `SpawnError` 787), the A6 section,
  the fault points section.
- Merged reports to read first (`ai_docs/reports/arch-refactor-dataset/`):
  `a6a-store.md` (record-based `ExecutionStore`; `Execution`,
  `to_execution`/`from_execution`, `find_by_idempotency`,
  `ExecutionStore::open` are NOT built — you build them; `set_skills`
  exists but is not called — you wire it), `a4-harness.md` (the
  capability-driven launch flow API that `run_worker` calls),
  `a7b-messaging.md` (`execution/lifecycle.rs` holds `done` and
  `ReportTarget`; add `run_worker` next to it), `a5-routing.md`
  (`routing::decide` still returns the legacy `Decision`; switch the plan to
  `RoutingDecision` + `RoutingProvenance`), `a2-runtime.md` (`Settings.faults`
  has only `has()`: add `indexed` and `abort_if`), `a9a-skills.md`
  (`plan_activation`), `e2e-fakes.md` (fakes for all 5 harnesses,
  deterministic session ids, `fail_split`/`fail_run` scenarios; e2e exec
  commands need absolute paths; fake-prime has no long-lived daemon).
- Today: `crates/horch/src/cmd/spawn.rs` (501 lines: `SpawnArgs`, `Refused`,
  `spawn` at 80 with resume rules, routing, reserved-tier check, effort,
  skill support, role allocation, brief, split, run, tile) and
  `crates/horch/src/cmd/worker.rs` (498 lines: `worker`, `child_env`,
  `launch_agent`, `agent_command`, `run_agent`).
- Risk 5 decision (already applied by U25): `horch sessions` text keeps the
  legacy words; do not change prompt goldens.

## FILES

own:
- `crates/horch-core/src/execution/{plan,service}.rs` (new)
- `crates/horch-core/src/execution/model.rs` (add `Execution`, `SpawnRequest`, `ReportTarget` if not present, `ExecutionPlan`)
- `crates/horch-core/src/execution/lifecycle.rs` (add `run_worker`; keep `done` as U21 left it)
- `crates/horch-core/src/execution/store.rs` (add `Execution` conversion, `find_by_idempotency`, `open`)
- `crates/horch-core/src/execution/mod.rs`
- `crates/horch-core/src/runtime/fault.rs` (`indexed`, `abort_if`)
- `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/cmd/worker.rs`, `crates/horch/src/main.rs` (exit-code mapping only)
- `crates/horch-e2e/src/bin/fake-herdr.rs` (only if `fail_split`/`fail_run` need a fix)
- `crates/horch-core/tests/execution_plan.rs`, `crates/horch-e2e/tests/lifecycle.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/a6b-service.md`

do not touch: oracle and golden data, `harness/**` (call its flow; ask if
the API lacks something), `workspace/**`, `messaging/**`, `routing/**`.

## STEPS

1. Create the worktree (conventions §2).
2. Types: `Execution` per design §4.3 with `to_execution(&LedgerRecordV1)`
   and `from_execution(&Execution) -> LedgerRecordV1` (lossless for every
   legacy field); `SpawnRequest` per design plus `workdir: Option<PathBuf>`,
   `kind: ExecutionKind`, `report_to: ReportTarget`;
   `ExecutionPlan { execution, worker, launch, skills: SkillActivationPlan, workspace, gate_line }`;
   `SpawnError { Refused { .. }, Plan(PlanError), LaunchFailed { id, stage, source } }`
   with hand-written `Display`/`Error`.
3. `execution/plan.rs`: pure `plan_launch(req, inputs) -> Result<ExecutionPlan, PlanError>`
   moving the logic of `spawn.rs` lines 80 to about 262 (resume rules,
   routing via `RoutingDecision` with provenance, reserved-tier check,
   effort, skill support via `plan_activation`), and `finish_plan(plan, role, workspace)`.
   `PlanInputs` carries everything that was read (roster, quota view, the
   ledger records needed for resume, clock value, minted ids), so the
   function does no I/O. Role allocation stays impure in the service, under
   the store's `DirLock`; briefs are written atomically.
4. `execution/service.rs`: `ExecutionService::spawn(&self, plan) -> Result<SpawnOutcome, SpawnError>`
   in this order: insert record (`Planned`) → write brief → split pane
   (on error: set `LaunchFailed{Split}` and unregister) → run in pane (on
   error: `LaunchFailed{Run}`, close the pane, unregister) →
   `mark_starting(pane)` → tile best effort. Call `set_skills` after the
   insert (SKL-04). Fault points from `HORCH_FAULT` via `Faults::abort_if`:
   `abort-after-execution-insert`, `abort-after-brief`,
   `abort-after-pane-split`, `fail-pane-split`, `fail-pane-run`. The
   service talks to herdr through `WorkspaceClient`.
5. `execution/lifecycle.rs::run_worker` in the Spec A §8 order (read the
   design's A6 section for the order; if it is not listed, use: load brief →
   build context → register mailbox → set Running → launch through the
   harness flow → wait → record `agent_exited(code)` → done handling) and
   record `Failed(AgentExited{code})` for a non-zero exit of a worker kind;
   propagate the exit code.
6. Thin the CLI: `cmd/spawn.rs` (target about 80 lines) builds a
   `SpawnRequest` from args and context, gathers `PlanInputs`, calls
   `plan_launch` and the service, prints. `cmd/worker.rs` (target about 30
   lines) calls `run_worker`. `main.rs` maps `SpawnError::Refused` to exit 3
   by type (no string matching).
7. Tests:
   - `arc_15_plan_deterministic` (same inputs → equal plans),
     `arc_15_plan_table` (a table of requests × inputs → expected decision,
     teammate, effort, session mode, skills; include resume, substitution,
     refusal, reserved tier).
   - `arc_16_split_failure_launch_failed`, `arc_16_run_failure_closes_pane`
     (unit tests with `FakeWorkspace`), `arc_16_e2e_fail_split` (e2e with
     fake-herdr `fail_split`: exit non-zero, record state LaunchFailed,
     legacy status `done`, no live row), `arc_16_crash_after_insert_not_live`
     (`HORCH_FAULT=abort-after-execution-insert`: the record is not live
     afterwards; define and test the recovery rule for a Planned record
     with no pane).
   - `arc_18_worker_startup_order` (recorded call order),
     `arc_18_agent_exit_recorded` (fake agent exits 3 → state
     Failed{AgentExited{3}}, exit code 3).
   - `arc_26_e2e_lifecycle_matrix_claude`, `..._codex`, `..._opencode`,
     `..._pi`, `..._prime` in `crates/horch-e2e/tests/lifecycle.rs`: fresh
     spawn → session discovered (caller-minted or harvested) → `horch done`
     → `horch spawn --resume` reuses the session.
   - `bal_04`, `bal_05`, `bal_06` and every existing e2e stay green.
8. Gate after each step. Commits: `A6: Add Execution and SpawnRequest`,
   `A6: Pure spawn plan`, `A6: ExecutionService with typed launch failures`,
   `A6: Worker lifecycle`, `A6: Thin spawn and worker commands`,
   `A6: Add ARC-15, 16, 18, 26 tests`.
9. Write and commit the report. Follow conventions §6.

## DONE WHEN

- Every named test passes; `./scripts/check-req-coverage.sh --phase A6`
  exits 0 (with U25's ARC-22/ARC-23 tests merged).
- `wc -l crates/horch/src/cmd/spawn.rs crates/horch/src/cmd/worker.rs` is
  under about 150 and 80.
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: API for B3 (the coordinator spawns candidates
  through `ExecutionService` with `kind: Candidate`, `workdir`,
  `report_to: None`), the recovery rule, gotchas.
