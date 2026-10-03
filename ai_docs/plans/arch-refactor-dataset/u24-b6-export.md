# U24 b6-export: dataset export rows, readiness report, outcomes

Unit slug: `b6-export`. Branch: `ard/b6-export`. Phase: B6 (core part).
Requirements: EXP-03, EXP-04, EXP-05, EXP-06.

## GOAL

The dataset can be exported as System-One/Laya-shaped JSONL rows regenerated
only from recorded data (events, judgments, receipts, execution facts,
outcomes), byte-identically on every run; a readiness report says whether the
local data justifies enabling Clef or Laya and lists the gaps; and an
operator outcome (`regression | revert | verified`) is recorded as an event
and appears in the export.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: OD4 (Clef and Laya inert; readiness decides),
  §2 "export row", §3 "B6", §4 EXP rows.
- Design doc `ai_docs/designs/2026-10-02-dataset-competition-design.md`
  §4.10 (`ExportRow`, `ExportState`, `export`, readiness types and
  `readiness`), §2.1 (`exports/<label_policy_version>/<ts>.jsonl`).
- Merged code and reports (`ai_docs/reports/arch-refactor-dataset/`):
  `b1-measure.md` (events incl. `OutcomeRecorded`, `JsonlRecorder`,
  `read_all`, `fold`, `RoundView`, `WorkerRun::project(ExecutionFacts, …)`,
  `DatasetPaths`), `machine-teacher.md` (`teacher::system_one::{Question, QuestionKind, Answer, API}`,
  `TeacherRef::none()`), `b4-evaluation.md` (`JudgmentRecord`,
  `CandidateAssessment.scores`), `b1-primitives.md` (`MicroUsd`, digest,
  canonical JSON).
- Deviation: the execution store (`ExecutionStore`) is being built now by
  U19 `a6a-store`. `export` takes `&dyn ExecutionFactsSource` (a small trait
  you define: `facts(execution_id) -> Option<ExecutionFacts>`), with an
  in-memory implementation for tests. The CLI unit adapts the store later.
- Deviation: promotion receipts (B5) do not exist yet. Read
  `promotions/<round>.json` if present as `serde_json::Value` and include
  only `dest_after` in the row (`// B5: typed once PromotionReceipt lands`).
- Parallel units: U22 `b3-planner`, U23 `b4-judge-input`, and A-phase
  refactors. You own only new files.

## FILES

own:
- `crates/horch-core/src/dataset/{mod,export,readiness,outcome}.rs` (new)
- `crates/horch-core/src/lib.rs` (add `pub mod dataset;`)
- `crates/horch-core/assets/dataset-policy.json` (new; default thresholds)
- `crates/horch-core/tests/dataset_export.rs` (new)
- `crates/horch-core/tests/golden/export-1.0.0.jsonl` (new)
- `crates/horch-core/tests/fixtures/dataset/events/**` (new)
- `ai_docs/reports/arch-refactor-dataset/b6-export.md`

do not touch: every other file.

## STEPS

1. Create the worktree (conventions §2).
2. `dataset/export.rs`: types per design §4.10 and
   `export(paths, facts, label_policy_version) -> Result<Vec<ExportRow>>`:
   read all events (`read_all`), fold, and for every DECIDED, REJECTED,
   NEEDS_INTERVENTION or later round build one row:
   - `questions["best_worker"]`: `Question { kind: Choice, options: eligible config ids (sorted) }`;
     `questions["quality:<config_id>"]`: `Score` per candidate config.
   - `answers["best_worker"]`: the winner's config id (`Answer.choice`,
     probability 1.0 on it) or `null` on abstain, tie, reject or
     needs-intervention; `answers["quality:<id>"]`: the judge's component
     scores summed only for this projection; keep components separately in
     `component_quality` (never collapse them elsewhere).
   - `eligible_set`, `planner_propensities` from `round.created`;
     `teacher` = `TeacherRef::none()`; `worker_runs` via
     `WorkerRun::project`; `cost_microusd` summed in `NanoUsd` then rounded
     once; `latency_ms` = round created → decided.
   - `outcomes`: every `outcome.recorded` for the round.
   Sort rows by (experiment_id, round_id); serialize each row with sorted
   keys (canonical JSON) one per line. `write_export(paths, rows, ts)` writes
   `exports/<label_policy_version>/<ts>.jsonl` with `create_immutable`.
3. `dataset/readiness.rs`: types per design; `readiness(rows, thresholds)`;
   thresholds load from `assets/dataset-policy.json` (Clef: ≥ 50 judged
   rounds, ≥ 4 arms with ≥ 10 runs each, ≥ 30 distinct tasks; Laya: ≥ 500
   judged rounds and ≥ 50 runs per arm), overridable by a file the caller
   passes. `gaps` lists each unmet threshold with numbers ("judged rounds
   12/50"). The arm key string is `<teammate>|<harness>|<model>|<effort>`.
4. `dataset/outcome.rs`: `record_outcome(recorder, experiment, round, kind, post_merge_score, note, at)`
   appends `outcome.recorded` with idempotency key
   `outcome:<round>:<kind>:<at>`; validates `post_merge_score` is finite in 0..=1.
5. Fixture: a hand-written event log under
   `crates/horch-core/tests/fixtures/dataset/events/` (or generated in the
   test through `JsonlRecorder` with fixed ids and times) covering 3 rounds:
   one winner, one tie (needs intervention), one all-failed (rejected),
   with judgments for the first two and one outcome.
6. Tests in `crates/horch-core/tests/dataset_export.rs`:
   - `exp_03_row_shape`: keys and value types of a row equal design §4.10;
     `schema` is `mh.export/1.0.0`, `api` is `systemone/v1`, `teacher` is
     `{"id":"none","probabilities":null}`.
   - `exp_04_export_golden`: the export of the fixture equals
     `crates/horch-core/tests/golden/export-1.0.0.jsonl` (bless once with
     `HORCH_BLESS=1`, review, commit; never re-bless).
   - `exp_04_regenerate_twice_identical`: two exports → identical bytes.
   - `exp_05_coverage_and_verdict`: synthetic rows below, at and above the
     Clef thresholds give NotReady with gaps, ClefReady, LayaReady.
   - `exp_06_outcome_recorded_and_exported`.
7. Gate after each step. Commits: `B6: Add export rows`, `B6: Add readiness report`,
   `B6: Add outcome recording`, `B6: Add EXP tests and export golden`.
8. Write and commit the report. Follow conventions §6.

## DONE WHEN

- The named tests pass. `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: API, deviations, gotchas for the CLI unit.
