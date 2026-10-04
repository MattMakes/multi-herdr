# Report: T1 spec-a-core (Spec A §3, §4, §8, §16, §17)

Unit `spec-a-core`, branch `ds/spec-a-core`, worker opus-51, 2026-10-04.
Plan: `ai_docs/plans/finish/t1-spec-a-core.md`.

The original Spec A text is not available. Each closed marker now states
the implemented, tested behaviour as the spec, and cites code and test.
Design file: `ai_docs/designs/2026-10-02-architecture-refactor-design.md`
(below: "the design").

## Closed markers

| # | Location (before) | What it says now | Evidence | Code changed |
|---|---|---|---|---|
| 1 | design line 15 (marker convention) | The finish run closed every spec placeholder marker; each place states tested behaviour and cites code and test. | this report | no |
| 2 | design §1.3, `SPEC-RESOLVED(Spec A §16)` | The 15 junior checklist items. Item 12 is now "no re-export shim" (`arc_25_no_shim_modules`), because A12 removed the shims. | `ai_docs/gates/architecture-refactor/CHECKLIST.md`, `scripts/phase-gate.sh`, `scripts/check-req-coverage.sh`, `scripts/check-deps.sh`, `nfr_05`/`nfr_06`/`nfr_09`/`nfr_11` (`horch-core/tests/nfr.rs`) | no |
| 3 | design §2.4, `SPEC-RESOLVED(Spec A §3)` | 14 module rules. Each row names the real scan test, its file, and the exact needles and file set. The CLI rule allows a call to a pure core function (`horch route` calls `routing::decision::decide`). | see "§3 scans" below | yes: 4 new scan tests |
| 4 | design §4.2, `SPEC-RESOLVED(Spec A §4)` (RuntimeContext) | `RuntimeContext{paths, herdr, bins, settings, inherited, worker}`, with every field and the variable behind it, `EnvSource`/`ProcessEnv`/`MapEnv`, `BinOverrides`/`HarnessBins` (10 tools, including `antigravity`), `Faults`. The abort exit code 86 is the spec. | `runtime/context.rs:RuntimeContext`, `arc_05_context_from_map_env`, `arc_05_no_ambient_env_in_core`, `runtime/fault.rs:ABORT_EXIT_CODE`, `arc_16_crash_after_insert_not_live` | no |
| 5 | design §4.3, `SPEC-RESOLVED(Spec A §4)` (Execution) | The 25 `Execution` fields with their ledger keys, and `Task{id, text, plan}`. 3 differences from the first sketch, each with its reason: text timestamps, `model: String`, no `worker` field. | `execution/model.rs:Execution`, `execution/store.rs:to_execution`/`from_execution`, `arc_17_execution_conversion_lossless` | comment only |
| 6 | design §4.5, `// SPEC-RESOLVED(Spec A §8)` (SpawnRequest) | The 14 `SpawnRequest` fields as the code has them (`resume`, `role`, `from_pane`, `direction`, `pinned`; no `session`, `balance`, `routing_mode`, `idempotency_key`). `ReportTarget` lives in `lifecycle.rs`. | `execution/model.rs:SpawnRequest`, `SpawnRequest::worker` | comment only |
| 7 | design §4.5, `SPEC-RESOLVED(Spec A §8)` (startup order) | `WorkerSteps` and the 7-step order: load_brief, enter_context, register, set_running, launch, agent_exited, return code (1 on a signal). Each step has its failure rule. | `execution/lifecycle.rs:run_worker`, `arc_18_worker_startup_order`, `arc_18_register_failure_records_failed`, `arc_18_enter_context_failure_records_failed`, `arc_18_agent_exit_recorded` | comment only |
| 8 | design §3.4, `SPEC-RESOLVED(Spec A §17)` | The 16 criterion texts are Spec A §17, as the master plan's table "Spec A §17 acceptance criteria (verbatim)" records them. The IDs' tests pin each row. | `00-master-plan.md` §4; `check-req-coverage.sh --through A12` exits 0 | no |
| 9 | `execution/model.rs:224` | Pointer: "The field list is Spec A §4 (architecture design §4.3)." | as row 5 | comment |
| 10 | `execution/model.rs:272` | Pointer: "The field list is Spec A §8 (architecture design §4.5)." | as row 6 | comment |
| 11 | `execution/lifecycle.rs:179` | Pointer: "The order is Spec A §8 (architecture design §4.5)." | as row 7 | comment |
| 12 | `execution/store.rs:94` (Spec B, judge key) | "A judge has no key of its own: `round_id` plus `judge:<attempt>` is its key (architecture design §4.3)." The decision is opus-52's (T2); the orchestrator gave this marker to me. | `store.rs:kind_of`, `arc_17_execution_conversion_lossless` | comment |

## §3 scans

The old table named tests that do not scan for 4 rules. I added a scan for
each and proved that each scan fails on a planted violation (then restored
the file):

| Test | File | Planted violation | Result |
|---|---|---|---|
| `arc_15_plan_is_pure` | `horch-core/src/execution/plan.rs` | `std::fs::metadata` in `plan.rs` | FAILED, then ok |
| `arc_23_telemetry_never_routes` | `horch-core/tests/arch_scan.rs` | `use crate::routing::decision::RoutingDecision` in `telemetry/readers.rs` | FAILED, then ok |
| `arc_10_prompts_hold_no_execution_policy` | `horch-core/tests/arch_scan.rs` | `use crate::routing::policy::BalanceMode` in `prompts.rs` | FAILED, then ok |
| `cmp_02_evaluation_domain_has_no_adapter_imports` | `horch-core/tests/arch_scan.rs` | `std::fs::metadata` in `evaluation/winner.rs` | FAILED, then ok |

The orchestrator gave me `arch_scan.rs` for these 3 scans (option 1). Test
names start with the lowercase requirement ID. I added the new names, and
the 2 existing scans `arc_08_roster_never_calls_herdr` and
`arc_13_routing_never_launches`, to the ARC-08, ARC-10, ARC-13, ARC-15 and
ARC-23 rows of the §3.1 table.

## Decisions

- The CLI rule forbids error-text matching and harness dispatch in the
  CLI. It does not forbid a call to a pure core function. Reason:
  `horch route` must show the same decision that `horch spawn` makes
  (BAL-06).
- The fault abort exit code stays 86. Reason: no `horch` or
  `multi-herdr-dataset` exit code uses it.
- I did not change runtime behaviour. The code was the truth for every
  marker.

## Outside my scope (not fixed)

- `ai_docs/gates/architecture-refactor/CHECKLIST.md` still says
  "provisional until the Spec A §16 text arrives", and its shim line says
  "Every moved module left a re-export shim (until A12)". The design §1.3
  now states the items. T6 can align the file.
- The design header table says "Spec A verbatim is pending (Appendix A)"
  and "Source of truth ... until Appendix A holds Spec A". T6 owns the
  history text.
- Design §4.5 sketches of `LaunchPlan`, `ExecutionPlan`, `PlanInputs`,
  `PlanError` and `ExecutionService` were not markers. I did not check
  them against the code.
- `workspace/mod.rs` `arc_27` FORBIDDEN lists `crate::workspace::herdr`
  twice. This is harmless.
- `runtime/fault.rs` says "A design choice: no spec names one." for
  `ABORT_EXIT_CODE`. The design §4.2 now makes 86 the spec.
- `CMP-02` in the dataset design does not list
  `cmp_02_evaluation_domain_has_no_adapter_imports`. The ID still has
  tests, so coverage passes.
- Markers that stay, not mine: design `SPEC-RESOLVED(Spec B)` (judge key,
  T2 writes it in §4.3), `SPEC-RESOLVED(Spec A §10)` (manifest keys),
  `SPEC-RESOLVED(Spec A §13)` (compatibility list).

## Checks

- Full gate (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`) was
  green on the §3, §4 and §8 commits, before the rule change.
- §16 and §17 changed design text only: `check-req-coverage.sh --through
  A12` exits 0.
- Before READY-TO-MERGE: the targeted checks of the rule change 2.
