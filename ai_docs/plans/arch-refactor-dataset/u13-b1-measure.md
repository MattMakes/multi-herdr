# U13 b1-measure: events, store, recorder, projection, WorkerRun, dataset paths

Unit slug: `b1-measure`. Branch: `ard/b1-measure`. Phase: B1 (rest).
Requirements: MEA-01 (ids part), MEA-02, MEA-03, MEA-04, MEA-05, MEA-06,
MEA-08, MEA-11, SEC-02, CMP-02.

## GOAL

The dataset measurement layer exists and is tested: a typed append-only event
log under a dataset dir, an idempotent JSONL recorder with a no-op twin, a
deterministic projection fold, the WorkerRun 1.0.0 projection with a golden,
the dataset path layout, an allowlisted environment snapshot, and the pure
competition domain types. Nothing in normal `horch` mode calls it yet.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: OD3, "Hard constraints", §2 "measure", §3 "B1",
  §4 MEA, SEC-02, CMP-02 rows.
- The design doc is your main spec:
  `ai_docs/designs/2026-10-02-dataset-competition-design.md` sections 2
  (storage layout, permissions, write disciplines), 4.1 (events), 4.2
  (recorder, store, projection), 4.3 (WorkerRun 1.0.0), 5 (round state
  machine, for the projection's transition check), 6 "B1".
  Where the design and this plan differ, the design wins, except where this
  plan says "deviation".
- Merged building blocks you must use (read
  `ai_docs/reports/arch-refactor-dataset/b1-primitives.md` and
  `a1-vocabulary.md`):
  - `horch_core::fsx`: `write_atomic`, `replace_durable`, `create_immutable`,
    `ensure_private_dir`, `PRIVATE_FILE`, `PRIVATE_DIR`,
    `DirLock::acquire(dir, name, stale_after, timeout)`.
  - `horch_core::measure::{digest::{Digest, digest_json, canonical_json}, testkit::{SplitMix64, property}, redact}`.
  - `horch_core::ids` (`EventId::mint`, `ExperimentId`, `RoundId`,
    `ExecutionId`, `JudgmentId`, `TaskId`, ...), `horch_core::execution`
    (`FailureKind`, `ExecutionStatus`), `horch_core::harness::HarnessKind`,
    `horch_core::teacher::TeacherRef`.
  - `horch_core::telemetry::cursor::poll_lines` (read new lines of a file
    from an offset; read its signature).
- Deviation (types that other units build now): `EligibleEntry` and
  `RoutingProvenance` (A5, in flight), `RejectReason` (U15 `b4-evaluation`
  owns it in `evaluation/winner.rs`), `PreflightReport` (B2),
  `ValidationReport` and `NumstatLine` (B3), `PromotionStrategy` (B5) do not
  exist yet. In event payloads, type those fields as `serde_json::Value`
  with a comment `// B<n>: typed once <type> lands`. Every other payload
  field is typed as the design says.
- Deviation (faults): `runtime::fault::Faults` comes from A2 (in flight). Take
  a plain `StoreOptions { abort_after_append: bool }` in `JsonlRecorder::open`
  instead; the B3 coordinator maps `HORCH_FAULT=abort-after-event-append`
  to it later.
- Deviation (Execution): the `Execution` struct comes in A6. `WorkerRun`
  projects from a local input struct `ExecutionFacts` (the fields of design
  §4.3 that come from the execution record) plus events. A6/B3 adapt later.
- Parallel units: U10 `a2-runtime`, U11 `a3-roster`, U12 `a5-routing`, and
  other wave 3 units (`b2-vcs`, `b4-evaluation`, `a9a-skills`,
  `b2-preflight`). You own only new files plus `measure/mod.rs` lines.

## FILES

own:
- `crates/horch-core/src/measure/{event,store,recorder,projection,worker_run,paths,envsnap}.rs` (new)
- `crates/horch-core/src/measure/mod.rs` (add your `pub mod` lines)
- `crates/horch-core/src/competition/mod.rs` (new; only `pub mod model;`)
- `crates/horch-core/src/competition/model.rs` (new)
- `crates/horch-core/src/lib.rs` (add `pub mod competition;`)
- `crates/horch-core/tests/measure.rs` (new)
- `crates/horch-core/tests/golden/worker-run-1.0.0.json` (new; see CONSTRAINTS)
- `crates/horch-core/tests/fixtures/measure/**` (new)
- `ai_docs/reports/arch-refactor-dataset/b1-measure.md`

do not touch: every other file.

## STEPS

1. Create the worktree (conventions §2).
2. `measure/paths.rs`: `DatasetPaths::new(state_root: &Path, project_slug: &str)`
   rooted at `<state_root>/multi-herdr/<project-slug>/` (OD3), with accessors
   for every path in design §2.1 (`events_dir`, `events_file(date)`,
   `experiment_dir(id)`, `manifest(id)`, `round_file(exp, round)`,
   `artifacts_dir(exp)`, `judgement(round)`, `promotion(round)`, `exports_dir`,
   `jobs_dir(round)`, `worktrees_dir(exp)`). `ensure()` creates the dirs 0700.
   Test `mea_08_dataset_dir_not_read_as_ledger`: create a state root with a
   dataset dir holding event files and a valid-looking `.json`, then run the
   telemetry ledger reader (`telemetry::collect::read_ledgers` or the
   function it uses; find it) and assert it reads no dataset file.
3. `measure/event.rs`: everything in design §4.1. `EventKind::from_parts`
   maps an unknown kind to `Unknown` and keeps the payload byte-for-byte.
   `occurred_at` formats as RFC 3339 UTC with exactly 3 fractional digits.
   Tests: `mea_02_envelope_roundtrip_every_kind` (one sample per kind,
   envelope → JSON → envelope equals), `mea_02_unknown_kind_preserved`,
   `mea_01_ids_v7` (every minted `EventId` is v7 and sorts by time).
4. `measure/store.rs` + `measure/recorder.rs`: design §4.2.
   - Files `events/YYYY-MM-DD.jsonl` (UTC date of `occurred_at`), 0600,
     one envelope per line, `\n` terminated, appended under
     `DirLock` `events.lock` in the events dir, `sync_data` after each append.
   - On open, rebuild the idempotency index (key → envelope) by reading all
     files; skip and count torn lines (a last line without `\n` or with
     invalid JSON). Under the lock, before each append, refresh the index
     from the bytes appended by other processes since the last read
     (`telemetry::cursor::poll_lines` or an offset per file).
   - A duplicate key returns `Appended::Duplicate(earlier)` and writes nothing.
   - `NoopRecorder` returns `Recorded` and writes nothing.
   - Tests: `mea_03_torn_line_skipped`, `mea_03_two_process_appends_ordered`
     (spawn 2 threads or 2 child processes of the test binary that append
     200 events each with distinct keys; all 400 are read back, no torn
     line, per-writer order kept), `mea_04_duplicate_key_noop`,
     `mea_04_prop_duplicates_no_effect` (testkit `property`, 10 000 cases,
     fixed seed: a random sequence with repeated keys yields the same
     read-back as the deduplicated sequence), `sec_05` is already covered
     by fsx; add `measure_files_are_private` for the 0600/0700 modes.
5. `measure/projection.rs`: `fold(events) -> Projection` per design §4.2 and
   the state machine in §5 (use the states that §5 defines; the transition
   table itself is CMP-03 in B3, so here implement only what fold needs:
   apply a valid transition, record an `Anomaly` for an invalid one, never
   apply it). Tests: `mea_05_prop_fold_deterministic` (same events → equal
   projection, 10 000 random event sequences, fixed seed),
   `mea_05_prop_replay_identical` (fold of a prefix then the rest equals fold
   of all, if your fold is incremental; otherwise fold twice equals),
   `mea_05_rebuild_equals_live` (append through `JsonlRecorder`, fold the
   live list, reopen and fold `read_all`, equal).
6. `measure/worker_run.rs`: WorkerRun 1.0.0 exactly as design §4.3, built from
   `ExecutionFacts` plus the events for that execution. No winner field.
   Facts and scores are separate fields. Tests: `mea_06_worker_run_schema`
   (the serialized keys equal the design list), `mea_06_no_winner_field`.
7. `mea_11_fake_lifecycle_replays_identical_worker_run`: a scripted fake
   lifecycle (planned → spawned → completed → frozen → validated) for one
   candidate with fixed ids and times, appended through `JsonlRecorder`,
   then reopened and projected to a WorkerRun, serialized with sorted keys
   and compared to `crates/horch-core/tests/golden/worker-run-1.0.0.json`.
   Bless it once with `HORCH_BLESS=1` (same helper pattern as the oracle
   tests), review it, and commit it. Run twice without bless.
8. `measure/envsnap.rs`: `env_snapshot(vars: &BTreeMap<String,String>) -> BTreeMap<String,String>`
   keeps only an allowlist (`LANG`, `LC_ALL`, `TERM`, `SHELL`, `TZ`,
   `HORCH_BALANCE`, `HORCH_TEAMMATES_DIR` presence only as `"set"`, the
   names of `HORCH_*_BIN` with values replaced by `"set"`) and passes every
   kept value through `redact`. It never reads the process env (the caller
   passes the map). Test `sec_02_env_snapshot_allowlist`: a map with
   `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `AWS_SECRET_ACCESS_KEY`,
   `GITHUB_TOKEN`, `PATH`, `HOME`, `LANG` keeps only `LANG`.
9. `competition/model.rs`: pure domain types from the master plan §1 and
   design §4 (Experiment, Round, Candidate with its anonymous label,
   ValidationRun, Judgment reference, Promotion, Outcome, `RoundState` per
   design §5). No I/O. Test `cmp_02_domain_has_no_adapter_imports`: scan
   `competition/model.rs` and `measure/{event,projection,worker_run}.rs`;
   none contains `std::process`, `Command`, `herdr`, `workspace::`,
   `launch::`, `vcs::`, `std::env`, or `std::fs` (store.rs and recorder.rs
   are adapters and are exempt).
10. Gate after each step. Commits: `B1: Add dataset paths`, `B1: Add event envelope and kinds`,
    `B1: Add JSONL event store and recorders`, `B1: Add projection fold`,
    `B1: Add WorkerRun 1.0.0 and golden`, `B1: Add env snapshot allowlist`,
    `B1: Add competition domain model`.
11. Write and commit the report (public API, deviations, every `serde_json::Value`
    placeholder field and which unit types it). Follow conventions §6.

## CONSTRAINTS

- The WorkerRun golden is a serialization golden: once committed it is never
  re-blessed. A later format change bumps `schema_version`.
- Property tests use `measure::testkit::property` with fixed seeds and 10 000
  cases; keep each under 2 s in debug builds (reduce event sizes, not cases).
- No `std::env` in your files.

## DONE WHEN

- `./scripts/check-req-coverage.sh --phase B1` reports every B1 ID ok
  (MEA-01..09, MEA-11, SEC-01, SEC-02, SEC-05, CMP-02, NFR-11; several are
  already covered by U06).
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: API, deviations, placeholder fields, gotchas for B2/B3.
