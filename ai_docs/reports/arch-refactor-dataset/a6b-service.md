# U26 a6b-service: report

Phase A6, commits 2 to 4. Requirements ARC-15, ARC-16, ARC-18, ARC-22
(exit-code part), ARC-26, SKL-04 (wiring). Branch `ard/a6b-service`.

## Module map

| File | Holds |
|---|---|
| `execution/model.rs` | `Task`, `Execution`, `SpawnRequest` (+ `SpawnRequest::worker`), `LaunchPlan`, `WorkspacePlan`, `ExecutionPlan`; re-exports `ReportTarget` and `HistoryEntry`; `PartialEq for HistoryEntry` |
| `execution/store.rs` | `to_execution`, `from_execution`, `LegacyError`, `ABANDONED_AFTER`; `ExecutionStore::{open, load, insert_execution, find_by_idempotency, mark_starting, record_exit, recover_abandoned}` |
| `execution/plan.rs` | pure `plan_launch`, `finish_plan`, `needs_gate`, `resolve_phase`, `PlanInputs`, `GateInputs`, `MintedIds`, `PlanError`, `IDLE_TASK` |
| `execution/service.rs` | `ExecutionService::spawn`, `SpawnError`, `SpawnOutcome` |
| `execution/lifecycle.rs` | `run_worker`, `WorkerSteps`, `PaneWorker`, `child_env` (+ `done`, unchanged) |
| `runtime/fault.rs` | `Faults::indexed`, `Faults::abort_if`, `ABORT_EXIT_CODE = 86` |
| `harness/launch.rs` | `run_flow_code` (additive; `run_flow` wraps it) |
| `cmd/spawn.rs` | reads inputs, calls plan and service, prints (175 lines without tests) |
| `cmd/worker.rs` | 27 lines: `run_worker(PaneWorker)` with a `set_current_dir` hook |
| `main.rs` | `SpawnError::Refused` → exit 3 by type (`cmd::spawn::Refused` is gone) |

## API for B3

```rust
let mut req = SpawnRequest::worker(Some(teammate), task);
req.kind = ExecutionKind::Candidate { experiment, round, label };
req.workdir = Some(worktree);          // the agent starts here
req.report_to = ReportTarget::None;
req.pinned = true;                     // no gate; provenance mode `pinned`
let plan = plan_launch(&req, &PlanInputs { roster, catalog, gate: None,
    existing: None, now, ids: &MintedIds { execution, session }, project })?;
let out = ExecutionService { ctx, store, workspace, mailbox, tile }
    .spawn(plan, Some(role))?;        // SpawnOutcome { plan, pane }
store.find_by_idempotency("spawn:<round>:<label>")?  // Option<Execution>
```

- `needs_gate(req, roster)` says whether the shell must read the quota view.
  It is false for a pinned or resumed spawn.
- `Execution::idempotency_key()` is `spawn:<round>:<label>` for a candidate.
  It is derived from `round_id` and `label`, so no new ledger key exists.
  The service does not check it. B3 calls `find_by_idempotency` before it
  plans.
- `report_to` is in the plan but not in the brief: `Brief` is in
  `messaging/` (not my file). `horch done` still reports to the
  orchestrator. B3 must carry `report_to` to the worker (a Brief field) if
  candidates must not report.
- A Judge record is written as `round_id` plus `label = "judge:<attempt>"`.
  This is `SPEC-RESOLVED(Spec B)`. (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3-§11, ai_docs/reports/finish/spec-b-preflight.md)

## Order and the recovery rule

- Service: `recover_abandoned` → insert `Planned` (role allocated under the
  ledger lock) → `set_skills` → brief (atomic, `fsx::write_atomic`) →
  split → run → `mark_starting(pane)` → tile.
- Failure: a brief or split failure sets `LaunchFailed{stage}` and
  unregisters the role. A run failure also closes the pane.
- Recovery rule: a `Planned` record with no pane whose `updated_at` is 5
  minutes (`ABANDONED_AFTER`) or more old becomes
  `LaunchFailed{Split, "abandoned: ..."}` on the next `horch spawn` in the
  project. Its role is unregistered when it is in the spawner's workspace.
  A `Planned` record is never live (`is_live` = Starting | Running). Its
  legacy `status` is `working` until recovery, so `--resume` refuses it.
- Worker (`run_worker`): load brief → enter context → register → set
  `Running` → launch through `run_flow_code` → record exit. Exit 0 sets
  `Done`. A non-zero code or a signal sets `Failed{AgentExited{code}}`. An
  orchestrator keeps its state. A record that is already terminal (after
  `horch done`) only gets `exit_code`. Ledger write errors are logged, and
  the agent still starts. `SPEC-RESOLVED(Spec A §8)`: the order verbatim. (closed: ai_docs/designs/2026-10-02-architecture-refactor-design.md §1.3-§4.5, ai_docs/reports/finish/spec-a-core.md)

## Decisions

- Orchestrator decision 1: `run_flow_code` in `harness/launch.rs`. It
  returns `Result<Option<i32>>`, not `Result<i32>`: `None` is a signal, and
  `FailureKind::AgentExited` takes `Option<i32>`. Nothing else in
  `launch.rs` changed (`run_agent` became `run_agent_code` plus
  `exit_code`).
- Orchestrator decision 2: fake-codex scenario `stay`, as a separate
  function. A fresh launch writes
  `$HOME/.codex/sessions/2026/10/02/rollout-...-<uuid>.jsonl` for its
  canonical cwd. The uuid comes from `hash16(cwd)` and `hash16("codex")`.
  A `resume <id>` launch records `resumed`. Both then sleep until killed.
- `SpawnRequest` differs from the design: `resume: Option<String>` (the
  record or session key) in place of `session: SessionMode`, and
  `pinned: bool` in place of `routing_mode`. It also has `role`,
  `from_pane` and `direction`. No `balance` field: the shell passes
  `GateInputs { view, balance }`. No `idempotency_key` field (see above).
- `LaunchPlan` is `{ teammate, model, session, task }`. The argv is not
  pre-built: the worker builds it through the harness flow, as A4 left it.
- `plan_launch` returns `PlanError`. A refusal is `PlanError::Refused`.
  `From<PlanError> for SpawnError` turns it into `SpawnError::Refused`.
  `SpawnError` has a 4th variant, `Store`, for ledger or mailbox errors
  before the launch. `SpawnError` has no `source()`, because its `Display`
  already contains the cause and `{e:#}` would print it twice.
- `PlanError` keeps every message `horch spawn` printed before.
- `Execution` keeps the ledger's timestamp text and `via` and
  `substitution_reason`, and has `typed_status: bool`. This makes
  conversion lossless: `arc_17_execution_conversion_lossless` round-trips
  the 4 A0 ledger oracles byte for byte. A `session_id` of `""` is
  `SessionState::Unavailable`.
- The fault point is `fail-pane-run`, as in the unit plan. The design calls
  it `fail-run`.
- I wrote the brief in the service with `fsx::write_atomic`, at the same
  path `Mailbox::write_brief` uses (`<role>.brief.json`).
  `Mailbox::write_brief` is not atomic, and `messaging/` is not my file.
  `arc_16_service_apply_order` reads the brief back with
  `Mailbox::read_brief`.
- `IDLE_TASK` is in `execution/plan.rs` with the same text as the private
  one in `ledger.rs`.
- The commit plan listed 6 commits. I made 4 code commits plus this
  report. The model, plan, service and CLI changes share call sites, so
  they are 1 commit. Each commit passes the gate.

## Behavior changes (small, deliberate)

- An agent that exits by itself now ends its record (`Done` or `Failed`).
  Before, the record stayed `working` until `horch done`.
- A launch failure now writes `LaunchFailed` and frees the role. Before,
  the record stayed `working` (a phantom).
- `horch spawn` prints the NOTE or SUBSTITUTED line after the effort and
  skill checks pass, not before them. A REFUSED line prints as before.
- A record that does not convert to an `Execution` (an invalid id or an
  unknown agent) cannot be resumed (`NotResumable`). All 4 oracles convert.

## Tests added: 18

- `crates/horch-core/tests/execution_plan.rs` (11):
  - `arc_15_plan_deterministic`
  - `arc_15_plan_table` (20 rows: fresh, discovered session,
    substitution, `--exact`, refusal, `--force`, pinned, `--phase`,
    `--effort`, bad effort, unknown teammate, headless-only, resume,
    resume on the fallback, 4 not-resumable cases, reserved tier, nothing)
  - `arc_15_plan_record_and_finish`
  - `arc_16_service_apply_order`
  - `arc_16_split_failure_launch_failed`
  - `arc_16_run_failure_closes_pane`
  - `arc_16_planned_record_recovery`
  - `arc_17_execution_conversion_lossless`
  - `arc_18_worker_startup_order`
  - `arc_18_agent_exit_recorded`
  - `invalid_selection_is_refused_before_spawn_side_effects` (moved from
    `cmd/spawn.rs`, now through `plan_launch`)
- `crates/horch-e2e/tests/lifecycle.rs` (7):
  - `arc_26_e2e_lifecycle_matrix_{claude,codex,opencode,pi,prime}`
  - `arc_16_e2e_fail_split`
  - `arc_16_crash_after_insert_not_live`
- `runtime::fault::tests::faults_indexed_points_carry_their_index` (1
  more, a unit test).
- Moved unchanged: `arc_06_transport_env_applied_to_child` (to
  `execution/lifecycle.rs` with `child_env`, imports only) and
  `phase_override_wins_and_resume_keeps_recorded_selection` (to
  `execution/plan.rs`).

## Gotchas

- After the rebase onto U28: `plan_launch` calls
  `skills::ensure_supported_in(teammate, inputs.catalog)`, and `horch spawn`
  passes `roster.skill_catalog()` as `PlanInputs.catalog`. `plan_activation`
  uses the same catalog.
- U28 asked to name the skill bundle after the real execution id
  (`bundle_name` in `harness/launch.rs`). No change is needed: the spawn
  plan writes the execution id as the ledger `record_id`, and the brief and
  `DiscoveryTarget.record_id` carry that id. A resume reuses the record's
  id; `bundle_name` mints a fresh id only when that directory still exists.
- fake-codex keeps both U28's `inspect_skills` and this unit's `stay`.
  `inspect_skills` runs first, because `stay` never returns.
- `cmd/spawn.rs` is 242 lines: 175 of code and 67 of tests. Of the code,
  about 45 lines are `SpawnArgs` and `resolve_positionals`, which
  `main.rs` calls and which I kept unchanged.
- In the e2e matrix, fake-herdr reuses a pane id after `pane close`, so a
  resumed pane can have the same id as the first one.
- The claude and pi fakes ignore `stay` and exit at once. Their record is
  `done` (exit 0) before the test sees `running`.
- The new workdir entry (`PaneWorker` enters `brief.workdir`) has no e2e
  test, because the CLI cannot set `SpawnRequest.workdir`. B3 must cover it.
- `horch sessions` text is unchanged by this unit.

## Outside my scope (not fixed)

- `tel_02_fleet_writes_orchestrator_record` failed once in 6 full e2e runs
  ("timed out waiting for the orchestrator's claude"). When it fails it
  leaves a `horch telemetry` collector running. Leaked collectors from
  other worktrees (`a9a-skills`, `b4-judge-input`) show the same, so the
  flake predates this unit.
- `LedgerRecordV1`/`HistoryEntry` derive no `PartialEq`. I implemented it in
  `model.rs` instead of editing `legacy.rs`.

## Checklist

- [x] `just gate` is green (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN`).
- [x] `scripts/check-req-coverage.sh --phase A6` lists every ID as ok.
- [x] Every new test name starts with its lowercase requirement ID (except the moved `invalid_selection_...`).
- [x] No golden prompt changed.
- [x] No serialization golden re-blessed.
- [x] No oracle regenerated.
- [x] No existing test changed to make it pass (3 tests moved with their code).
- [x] No new crate.
- [x] No new `std::env` read in core (`set_current_dir` stays in `cmd/worker.rs`).
- [x] No `ANTHROPIC_API_KEY` reaches a child (the e2e fakes report no violation).
- [x] Tests are hermetic.
- [x] Old ledgers and briefs still load.
- [x] The diff was re-read adversarially.
