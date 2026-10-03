# U24 b6-export: report

Branch `ard/b6-export`. Phase B6 (core part). Requirements EXP-03, EXP-04,
EXP-05, EXP-06. Nothing calls these modules yet; the CLI unit wires them.

## Public API

### `horch_core::dataset::export`
- `EXPORT_SCHEMA = "mh.export/1.0.0"`, `BEST_WORKER = "best_worker"`,
  `QUALITY_PREFIX = "quality:"`, `SCORE_KEY = "score"`.
- `ExportRow` (design §4.10 fields plus `promotion`), `ExportState`,
  `ExportPromotion { dest_after }`.
- `trait ExecutionFactsSource { fn facts(&self, &ExecutionId) -> Option<ExecutionFacts>; }`
  and `InMemoryFacts` (`insert`, `FromIterator<ExecutionFacts>`).
- `export(paths, &dyn ExecutionFactsSource, label_policy_version) -> Result<Vec<ExportRow>>`.
- `render_jsonl(rows) -> Result<String>`: one `canonical_json` line per row.
- `write_export(paths, label_policy_version, rows, ts) -> Result<PathBuf>`:
  `exports/<lpv>/<YYYYMMDDTHHMMSS.mmmZ>.jsonl`, `create_immutable`, 0600.

### `horch_core::dataset::readiness`
- `DEFAULT_POLICY` (`include_str!` of `assets/dataset-policy.json`).
- `ArmKey { teammate, harness, model, effort }`, `ArmKey::of(&RunConfig)`,
  `ArmKey::key()` = `<teammate>|<harness>|<model>|<effort or ->`.
- `ArmCoverage`, `ReadinessThresholds` (`from_policy_json`, `builtin`,
  `load(Option<&Path>)`), `ReadinessVerdict { NotReady, ClefReady, LayaReady }`,
  `ReadinessReport`, `readiness(rows, &thresholds)`.

### `horch_core::dataset::outcome`
- `record_outcome(&dyn Recorder, &ExperimentId, &RoundId, OutcomeKind, post_merge_score, note, at) -> Result<Appended>`.
- `OutcomeError::ScoreOutOfRange(f64)`.

## Tests added: 8 (`crates/horch-core/tests/dataset_export.rs`)

exp_03_row_shape, exp_03_other_label_policy_versions_are_not_exported,
exp_03_promotion_receipt_gives_dest_after, exp_04_export_golden,
exp_04_regenerate_twice_identical, exp_04_round_id_with_a_path_is_refused,
exp_05_coverage_and_verdict, exp_06_outcome_recorded_and_exported.

The golden `tests/golden/export-1.0.0.jsonl` (3 rows) was blessed once with
`HORCH_BLESS=1` and reviewed. The test never overwrites an existing golden.
A mutation check (outcomes dropped, non-canonical JSON) failed 3 tests; a
removed round-id guard failed the forged-round test.

## Decisions and deviations

- **Fixture.** The test builds the event log through `JsonlRecorder` with
  fixed ids and 1-second steps. There is no hand-written file under
  `tests/fixtures/dataset/events/`. Event ids are random per build; the
  export never contains them, and `exp_04_regenerate_twice_identical`
  compares 2 separate builds.
- **Execution facts.** `export` takes `&dyn ExecutionFactsSource`, not the
  `ExecutionStore` (plan deviation; U19 builds the store).
- **Rows per label policy.** `export` returns only rounds whose
  `round.created.label_policy_version` equals the argument.
- **Decided rounds.** A row exists for a round in DECIDED, REVALIDATING,
  PROMOTING, PROMOTED, NEEDS_INTERVENTION, REJECTED, CLEANUP or COMPLETE.
- **`latency_ms`.** From the `round.created` time to the time of the first
  event that put the round in one of those states. `export` folds one
  event at a time to find it.
- **`best_worker` options.** The config ids of the candidates whose
  validation is eligible, sorted. This is the `eligible` set of
  `decide_winner`. A round with no eligible candidate has no `options` key.
- **Winner.** It is the winner's config id when `winner.selected` exists and
  no later `winner.rejected` exists. A promotion conflict after the winner
  keeps the winner. Answer: `choice` = config id, `probabilities` =
  `{config: 1.0}`, `confidence` = null.
- **Quality answers.** `{"choice":null,"probabilities":{"score":<sum>},"confidence":null}`.
  The sum exists only there. `component_quality` keeps every component.
  Without a judgment the answer is null. `SPEC-TODO(System One score answers)`.
- **Duplicate config ids** in one round are an error, because the
  `quality:<config_id>` keys would collide. The B3 planner keeps them distinct.
- **`task_features`.** `budget_usd_micro`, `candidates`, `round_index` and
  `strategy`. `SPEC-TODO(Spec B export state)`.
- **`promotion`.** A field that the design does not have:
  `{"dest_after": …}` from `promotions/<round>.json` read as a JSON value,
  else null. `// B5: typed once PromotionReceipt lands`. A receipt without
  `dest_after` is an error.
- **Judgment file.** When the log has a `judgment_id`, the file
  `judgements/<round>.json` must exist and name the same judgment and round.
  Otherwise `export` fails. A file without `judge.completed` is ignored.
- **Missing facts.** A candidate without an execution id or without facts
  fails the export with a message that names the round.
- **`write_export` signature.** It takes `label_policy_version` explicitly,
  so an empty export has a directory. Every row must match it.
- **Readiness.** A row is judged when `component_quality` is not empty.
  `distinct_tasks` counts all rows. `gaps` lists the unmet thresholds of the
  next level only, as `clef: judged rounds 12/50`,
  `clef: arms with >= 10 runs 2/4`, `clef: distinct tasks 5/30`,
  `laya: judged rounds 50/500`, `laya: arms with >= 50 runs 0/4`.
  `SPEC-TODO(Spec B readiness)`: Laya's "≥ 50 runs per arm" is read as
  "at least `clef_min_arms` arms with ≥ 50 runs".
- **Policy file.** `{"schema_version":"1.0.0","readiness":{…}}` with
  `deny_unknown_fields`. An override file replaces all the thresholds.
- **Outcome.** The idempotency key is `outcome:<round>:<kind>:<occurred_at>`
  (RFC 3339 millis). The actor is `operator`.

## Security self-review

- Trust boundary: the files under the dataset root (0700). `RoundId` allows
  `/` and `..`, and `DatasetPaths::judgement`/`promotion` join it into a
  path. `export` now refuses a round id that is not one plain path
  component before it reads a file (`exp_04_round_id_with_a_path_is_refused`).
  `write_export` checks the label policy version the same way.
- The export holds transcript paths and digests, never transcript text
  (SEC-03). The judgment rationale and notes are not exported.
- No `std::env` read, no network, no new crate.
- Not fixed (outside scope): `DatasetPaths` itself accepts any `RoundId`;
  B1 or A12 can validate ids at the path layer. The judgment and receipt
  reads have no size cap (same-user files).

## Gotchas for the CLI unit

- Adapt `ExecutionStore` to `ExecutionFactsSource`.
- An `outcome.recorded` for a round in REJECTED or NEEDS_INTERVENTION is
  appended but the fold records it as an anomaly, so it never reaches a row.
  The CLI should check the round state first and refuse.
- `WorkerRun::project` folds the events it gets. `export` passes only the
  round's events, so the cost is linear in the round size per run.
- The intermediate commits were built in a temp worktree with a shared
  `CARGO_TARGET_DIR`. That left stale paths in `target/`; `cargo clean -p`
  fixed it. Do not share a target dir across worktrees.

## Gate

`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`: GATE GREEN.
`check-req-coverage.sh --phase B6`: EXP-01, EXP-03 to EXP-06 ok; EXP-07
belongs to another unit.
