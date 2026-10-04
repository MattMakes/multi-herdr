# U13 b1-measure: report

Branch `ard/b1-measure`. Phase B1 (rest). Nothing in normal `horch` mode
calls these modules yet.

## Public API

### `horch_core::measure::paths`
- `DatasetPaths::new(state_root, project: &Path)` (slug = `ledger::slug`),
  `DatasetPaths::from_slug(state_root, slug)`. Root:
  `<state_root>/multi-herdr/<slug>/` (`DATASET_DIR = "multi-herdr"`).
- Accessors: `root`, `events_dir`, `events_file(NaiveDate)`,
  `events_lock_dir` (`<root>/events.lock`), `experiments_dir`,
  `experiment_dir`, `manifest`, `rounds_dir`, `round_file`, `artifacts_dir`,
  `judge_input_dir`, `validation_dir(exp, round, label)`, `judgements_dir`,
  `judgement`, `promotions_dir`, `promotion`, `exports_root`,
  `exports_dir(label_policy_version)`, `jobs_root`, `jobs_dir(round)`,
  `job_dir(round, attempt)` (`judge-<n>`), `worktrees_root`,
  `default_worktree_root(exp)`.
- `ensure()` creates `multi-herdr/`, the root and the 7 top-level dirs, each
  0700. Per-experiment dirs are created by their writers (use
  `fsx::ensure_private_dir` for each level).

### `horch_core::measure::event`
- `EventEnvelope` (design §4.1 field order), `Actor`, `EVENT_SCHEMA_VERSION`,
  `format_occurred_at` (RFC 3339 UTC, exactly 3 fractional digits, `Z`).
- `EventKind`: 26 known kinds + `Unknown { kind, payload }`.
  `EventKind::KNOWN`, `name()`, `try_payload()`, `payload()` (panics only on
  a non-UTF-8 path), `from_parts(kind, payload)`. `EventEnvelope::event()`.
- `EventError { BadPayload, Unserializable }`.
- Payload enums: `SlotKind`, `JudgeFailure` (`kind` tag), `PromotionIntent`
  (`"not_requested"` | `{"requested":{"target":…}}`), `OutcomeKind`,
  `InterventionSource`, `FinalOutcome`.

### `horch_core::measure::store` (adapter)
- `read_all(paths) -> ReadEvents { events, torn_lines }`, `event_files`,
  `parse_line`, `lock(paths)` (`DirLock::acquire(root, "events", 60 s stale, 30 s timeout)`),
  `append(paths, file, env)`, `EventIndex { refresh, get, insert, len }`.
- `StoreOptions { abort_after_append }`, `StoreOptions::from_faults(&Faults)`,
  `ABORT_AFTER_EVENT_APPEND = "abort-after-event-append"`.

### `horch_core::measure::recorder`
- `NewEvent`, `NewEvent::into_envelope` (mints a v7 `EventId` at
  `occurred_at`; an empty idempotency key is an error).
- `Appended { Recorded, Duplicate }`, `Appended::envelope`, `trait Recorder`.
- `NoopRecorder` (writes nothing), `JsonlRecorder::open(&DatasetPaths, StoreOptions)`,
  `paths()`, `read_all()`. `JsonlRecorder` is `Sync` (index behind a `Mutex`).

### `horch_core::measure::projection`
- `fold(&[EventEnvelope]) -> Projection`, incremental `Projection::apply`.
- `Projection { experiments, rounds, anomalies }`, `ExperimentView`,
  `RoundView`, `CandidateView` (`is_terminal`, `is_eligible`), `JudgeView`,
  `PromotionView`, `Anomaly { event_id, reason }`, `MAX_JUDGE_ATTEMPTS = 2`.

### `horch_core::measure::worker_run`
- `WorkerRun`, `RunConfig`, `RunFacts`, `RunScores` (all
  `deny_unknown_fields`, no winner field), `WORKER_RUN_SCHEMA_VERSION`.
- `ExecutionFacts` (the execution-record part) and
  `WorkerRun::project(&facts, &events, judge_components) -> Result<_, WorkerRunError>`.

### `horch_core::measure::envsnap`
- `env_snapshot(&BTreeMap<String,String>)`: keeps `LANG`, `LC_ALL`, `TERM`,
  `SHELL`, `TZ`, `HORCH_BALANCE` (redacted), and `HORCH_TEAMMATES_DIR` and
  `HORCH_*_BIN` as `"set"`. Never reads the process environment.

### `horch_core::measure::NumstatLine`
- A re-export of `vcs::git::NumstatLine`, so the domain modules do not name
  `vcs::` (CMP-02).

### `horch_core::competition::model`
- `RoundState` (16 states, `SCREAMING_SNAKE_CASE`), `Experiment`, `Round`,
  `CandidateLabel` (`A`..`Z`, `AA`..; `from_index`, validated serde),
  `BadLabel`, `Candidate`, `ValidationRun`, `JudgmentRef`, `Promotion`,
  `Outcome`. No I/O.

## Tests added (28, all in `crates/horch-core/tests/measure.rs`)

mea_01_ids_v7; mea_02_envelope_roundtrip_every_kind, mea_02_unknown_kind_preserved,
mea_02_occurred_at_is_utc_millis, mea_02_known_kind_bad_payload_is_an_error;
mea_03_torn_line_skipped, mea_03_two_process_appends_ordered;
mea_04_duplicate_key_noop, mea_04_prop_duplicates_no_effect;
mea_05_prop_fold_deterministic, mea_05_prop_replay_identical,
mea_05_rebuild_equals_live, mea_05_full_lifecycles_fold_without_anomalies,
mea_05_invalid_transition_is_an_anomaly_and_not_applied,
mea_05_round_needs_intervention_and_completion_checks;
mea_06_worker_run_schema, mea_06_no_winner_field;
mea_08_dataset_dir_not_read_as_ledger; mea_11_fake_lifecycle_replays_identical_worker_run;
sec_02_env_snapshot_allowlist, sec_02_env_snapshot_presence_and_redaction;
cmp_02_domain_has_no_adapter_imports; dataset_paths_layout,
noop_recorder_writes_nothing, empty_idempotency_key_is_refused,
measure_files_are_private, store_options_from_faults,
candidate_labels_are_anonymous_letters.

`./scripts/check-req-coverage.sh --phase B1`: every B1 ID is ok.

## Decisions and deviations

- The design wins over the plan: `DatasetPaths::new` takes the project path
  (plus `from_slug`), and the lock is `<root>/events.lock/`, not in the
  events dir.
- The orchestrator added 3 kinds, each marked `SPEC-TODO(Spec B event list)`:
  `round.needs_intervention {reason, source: judge|promotion|operator}`,
  `round.cleanup_started {}`, `round.completed {final_outcome}`.
- The orchestrator decided to type the placeholders before the golden was
  blessed. Typed: `PreflightReport`, `EligibleEntry`, `RoutingProvenance`,
  `NumstatLine`, `ValidationReport`, `GateResult`, `ResolvedSkillRef`,
  `RejectReason`. **The only `serde_json::Value` placeholder left is
  `PromotionStarted.strategy` (B5 `PromotionStrategy`).**
- `RunConfig.routing` is required, as in the design. It comes from
  `candidate.spawned`, else from `ExecutionFacts.routing`; with neither,
  `project` returns `WorkerRunError::NoRouting`.
- `judge_components` is a parameter of `WorkerRun::project`, because no event
  carries the judgment's scores (the judgment file does).
- Fold transitions (the full table is CMP-03 in B3):
  - `experiment.created` puts the experiment in PREFLIGHT. `preflight.completed` with `passed` → PLANNED; with `passed: false` it stays in PREFLIGHT until `experiment.aborted`.
  - The round starts in PLANNED. The last `candidate.planned` → PROVISIONING, the last `worktree.created` → RUNNING, the last terminal candidate → VALIDATING.
  - The last `validation.completed` with ≥ 1 eligible → JUDGING_BACKGROUND. With 0 eligible, the round waits for `winner.rejected`.
  - `winner.selected` → DECIDED, or REVALIDATING when promotion is requested. `promotion.started` → PROMOTING, `promotion.completed` → PROMOTED.
  - `judge.failed` on attempt 2, `promotion.conflicted` and `round.needs_intervention` → NEEDS_INTERVENTION.
  - `round.cleanup_started` → CLEANUP. `round.completed` → COMPLETE, and only when `final_outcome` matches the state that cleanup started from.
  - Unknown kinds are ignored.
- `mea_04_prop_duplicates_no_effect` runs 10 000 cases through `EventIndex`
  and every 500th case through a real `JsonlRecorder`. Reason: each append
  costs about 14 ms on macOS (F_FULLFSYNC in the `DirLock` owner write and in
  `sync_data`).

## Gotchas for B2, B3, B4

- `NumstatLine` (from `vcs::git`) serializes `deleted`. The design §4.3 says
  `removed`. The WorkerRun golden freezes `deleted`.
- The read order of `read_all` is day file, then line order. An event whose
  `occurred_at` falls on an earlier UTC day than an event before it goes to
  the earlier file. Use a monotonic clock for `occurred_at`.
- A torn tail is terminated with `\n` before the next append, so it stays
  1 skipped line and never absorbs the next event.
- `fold` requires `execution_id` on the `candidate.planned` or
  `candidate.spawned` envelope. `WorkerRun::project` finds a run by that id.
- `CandidateLabel` is in `competition::model`, but event payloads still carry
  labels as `String`. B3 can switch them; the JSON is the same.
- `JsonlRecorder::open` calls `DatasetPaths::ensure`.
- The golden test never overwrites an existing golden, even with
  `HORCH_BLESS=1`.

## Outside my scope (not fixed)

- `NumstatLine` belongs in a domain module, not `vcs::git`. Moving it would
  let `measure/mod.rs` drop its re-export.
- `fsx::sync_dir` is private. `measure/store.rs` has its own copy.
