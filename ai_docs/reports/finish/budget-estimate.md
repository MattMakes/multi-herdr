# Report: G2 budget-estimate (opus-70)

Plan: `ai_docs/plans/finish/g2-budget-estimate.md`. Finding F1 of
`ai_docs/reports/finish/acceptance-dataset.md`: PRE-09 projected a fixed
`DEFAULT_TOKEN_ESTIMATE` per candidate ($1.60 sonnet, $1.72 codex-terra),
so `--budget-usd 2` always failed with 2 candidates. Real spend was about
$0.07 per candidate.

## Outcome

PRE-09 now takes each candidate's expected tokens from the first source that
has them (`competition/budget.rs:resolve_estimate`):

1. `plan`: `PreflightPlan.expected_tokens` by label (the CLI leaves it empty).
2. `config model`: `budget.expected_tokens.models.<model>`.
3. `config all`: `budget.expected_tokens.all`.
4. `measured N runs`: earlier candidates of the same task id on the same
   model, when there are at least 3.
5. `default`: `DEFAULT_TOKEN_ESTIMATE`.

The PRE-09 detail names the source of each label, for example
`projected $0.340000 (estimates: A: config all, B: config all) plus judge
reserve $0.200000 is under the soft limit $1.600000`. `measured` has
`estimate_source` by label.

## Decisions

- **Key shape.** `budget.expected_tokens` has `all` and `models.<model>`.
  Each estimate has the 5 kinds that `usage::Price` prices (`input`,
  `cache_write_5m`, `cache_write_1h`, `cache_read`, `output`). A kind that
  is not given is 0. One total with a split was rejected. The LA sonnet runs
  wrote 12k 1-hour cache tokens at 2× input price, so a fixed split prices
  them wrong. `TokenEstimate` gained the 2 cache-write kinds.
- **Model key.** The key is the candidate's model id as the roster names it
  (`sonnet`), with an exact match. It is not the canonical price id.
- **Measured: N = 3, the maximum per kind over the latest 10 runs.** PRE-09
  checks a ceiling, so a mean is too low. With 3 to 10 samples, a high
  percentile is the maximum. The LA runs of one small task varied by 30 % in
  cache reads. 1 run can be an outlier. A run with 0 tokens (no transcript
  found) does not count.
- **Where the read happens.** `budget.rs` must not use `std::fs` (CMP-02
  test `cmp_02_planner_files_have_no_adapter_imports`). The pure fold is
  `budget.rs:measured_from_runs`. The I/O is
  `crates/horch/src/dataset/preflight.rs:measured_tokens`. It runs in
  `gather` and reads the event log (task id, planned model per candidate)
  and `observe::load_usage_records`. A dataset that cannot be read gives no
  measurement.
- **Task id.** `task_id_of` (new, in `dataset/preflight.rs`) is the one
  derivation, `task-<sha256 short12>`. `record` uses it too. `GatherInput`
  gained `task` (1 line in `dataset/run.rs`, which the orchestrator granted;
  opus-69 was told).
- **Validation.** Load rejects an unknown kind, a negative or non-integer
  count, an estimate of 0 tokens and an empty model name. serde gives the
  path, for example `budget.expected_tokens.all.input: invalid type:
  integer -5, expected u64`.
- **Config digest.** The key is skipped in serialization when it is empty, so
  a config without it keeps its old `config_digest`.
- `BudgetConfig` is no longer `Copy`, because it now holds a map. No caller
  needed the copy.

## Tests

- `pre_09_configured_estimate_fits_the_task` (DONE WHEN): 2 sonnet candidates
  with an LA-sized `all` estimate project $0.34 and pass at `--budget-usd 2`.
  Without the key, they project $3.20 with `default` and fail.
- `pre_09_estimate_source_order`: plan > config model > config all >
  measured (≥ 3 runs) > default. 2 measured runs fall back to the default.
- `measured_estimate_is_the_max_of_the_latest_runs`: window, per-kind max,
  empty runs left out.
- `config_expected_tokens`: parse of `all` and `models`, plus 7 rejections.
- `dataset::preflight::tests::measured_tokens_reads_the_same_task_only`
  (horch crate): a recorded dataset with 4 rounds of 1 task and 1 round of
  another. A candidate without a usage record is left out.

## Files

- `crates/horch-core/src/competition/budget.rs`
- `crates/horch-core/src/competition/config.rs` (the new key only)
- `crates/horch-core/src/competition/preflight.rs` (PRE-09 and `TokenEstimate`)
- `crates/horch-core/tests/preflight.rs`
- `crates/horch-core/tests/competition_planner.rs` (1 line, granted)
- `crates/horch/src/dataset/preflight.rs`
- `crates/horch/src/dataset/run.rs` (1 line, granted)
- `ai_docs/designs/2026-10-02-dataset-competition-design.md` §4.11, §4.11.1
- `docs/command-flow.md` (1 paragraph before the exit-code table)

## Not done (outside scope)

- The live budget does not use these sources yet. `UsageMeter::projected`,
  which gives the coordinator's committed spend, still uses
  `DEFAULT_TOKEN_ESTIMATE`. A round that passes PRE-09 on a small
  configured estimate can therefore stop launches early: the coordinator
  counts $1.60 committed per running sonnet candidate. The fix is to give
  `UsageMeter` the per-model estimates that preflight resolved. That needs
  `run.rs` (G1) to build the meter from the plan, and maybe
  `coordinator.rs`. This affects a wave whose committed default crosses the
  limit. A round of 2 candidates in 1 wave launches both before the check
  matters.
- No `docs/` page lists the `dataset.yaml` keys. The dataset design §4.11 is
  the reference.
