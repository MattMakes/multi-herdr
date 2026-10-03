# U35 worker-startup-failure: a worker that fails before launch records it

Unit slug: `worker-startup-failure`. Branch: `ard/worker-startup-failure`. Phase: A6 follow-up (ARC-18).

## GOAL

When `horch worker` fails after it loads its brief but before the agent
launches (enter context, register, set running), the execution record goes
to a terminal failed state at once, so no record stays `Starting` with a
dead worker until a deadline.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read `ai_docs/reports/arch-refactor-dataset/a6b-service.md` (worker
  order, recovery rule) and the Gotchas of `b3-coordinator.md` (a worker
  that dies before `set_running` leaves a `Starting` record with a live
  pane; only the deadline ends it).
- Code: `crates/horch-core/src/execution/lifecycle.rs` `run_worker` (about
  line 127): `enter_context` and `register` errors return with `?` and
  record nothing. A `set_running` error is only logged.
- Parallel unit: U32 `b5-cli` (it does not touch `lifecycle.rs`).

## FILES

own:
- `crates/horch-core/src/execution/lifecycle.rs` (`run_worker`, `WorkerSteps`, `PaneWorker`)
- `crates/horch-core/src/execution/store.rs` (only if a "startup failed" setter is needed)
- `crates/horch-core/tests/execution_plan.rs` (only the new tests; U26's file)
- `ai_docs/reports/arch-refactor-dataset/worker-startup-failure.md`

do not touch: every other file. Reuse an existing `FailureKind` if one fits
(for example `LaunchFailed{stage}` with a new stage, or `AgentExited{None}`);
if a new variant changes a frozen serialization golden, stop and send
`QUESTION:`.

## STEPS

1. Create the worktree (conventions §2).
2. In `run_worker`, after `load_brief` succeeds: an error from
   `enter_context` or `register` records a terminal failure for the brief's
   record (best effort; log a recording error), then returns the original
   error. Decide and document what a `set_running` failure does (the agent
   still starts today; keep that).
3. Tests (unit, with a scripted `WorkerSteps`):
   `arc_18_register_failure_records_failed`,
   `arc_18_enter_context_failure_records_failed`; the existing
   `arc_18_worker_startup_order` and `arc_18_agent_exit_recorded` stay
   green.
4. Gate. Commit `A6: Record a worker startup failure`. Write and commit the
   report. Follow conventions §6.

## DONE WHEN

- The new tests pass; `just gate` is green.

## REPORT

- `horch done` summary: the failure state chosen, the tests.
