# U30 b3-coordinator: report

Branch `ard/b3-coordinator`. Phase B3 (coordinator), plus the B4 follow-ups
the orchestrator moved here after U31 merged. Requirements CMP-04 (e2e),
CMP-05, CMP-07, CMP-09 (wiring), CMP-10, CMP-11, CMP-13 (B3 and B4 points),
CMP-14, CMP-15, MEA-10, SEC-03, SEC-08 (candidates), ARC-24, and
`jdg_e2e_round_decided`. `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`
is green. `check-req-coverage.sh --phase B3`: every ID ok.

## What landed

| File | Content |
|---|---|
| `teammates/_base/competition-candidate.md` | the candidate rules; `{task}` and `{worktree}` placeholders |
| `competition/coordinator.rs` | `Coordinator`, `RoundSpec`, `RoundOutcome`, `DatasetWorkspace`, `candidate_task`, `candidate_role`, `monotonic`, the fault point names |
| `competition/observe.rs` | `classify`, `Observed`, `deadline_passed`, `TelemetryUsage`, `UsageRecord` and its files |
| `competition/budget.rs` | `UsageSource`, `UsageMeter` (exact n$ per token, rounded once), `PRICE_TABLE_DATE` |
| `messaging/brief.rs` | `report_to` (serde default `orchestrator`) |
| `execution/service.rs` | 1 line: the brief gets `plan.report_to` (orchestrator-approved) |
| `execution/store.rs` | `ExecutionStore::render_text`, moved from `Ledger::render` (orchestrator-approved; output byte-identical) |
| `cmd/messaging.rs` | `horch done` reads `report_to` from its brief |
| `cmd/ledgercmd.rs`, `main.rs` | `horch sessions [--json] [--all]` hides `round_id` records (candidate and judge) unless `--all`. `horch ledger list` still shows all |
| `cmd/spawn.rs` | `--resume` refuses a candidate or judge record |
| `dataset/run.rs` | `run` and `resume` through the coordinator; `SavedRun` (`experiments/<exp>/run.json`) |
| `dataset/preflight.rs` | `record` takes `&Faults`; `abort-after-experiment-created` (orchestrator-approved) |
| `dataset/watch.rs`, `cli.rs`, `mod.rs` | `watch <exp> --state-dir --project`; `resume <exp>` |
| `dataset/status.rs` | one line per candidate |
| `dataset/export.rs` | `StoreFacts.usage`; `facts_of(id, record, usage)` fills tokens, cost, cost source and transcript ref/digest |
| fakes | candidate mode (`horch_e2e::candidate_spec`, `run_candidate`); fake-herdr lock, dataset exemption, `workspace close`; fake judge ranks eligible labels first |

## The coordinator loop

Every step reads the round from `fold(events)` and records an event with
an idempotency key, so `resume` re-enters the same loop:

| Step | Event | Key |
|---|---|---|
| plan | `round.created`, `candidate.planned` (with a minted execution id) | `round.created:<round>`, `plan:<round>:<label>` |
| worktrees | `worktree.created` | `worktree:<round>:<label>` |
| spawn | `candidate.spawned` | `spawn:<round>:<label>` (also the store key) |
| observe | `candidate.completed` / `candidate.failed` | `ended:<round>:<label>` |
| freeze | `candidate.frozen` | `freeze:<round>:<label>` |
| validate | `validation.completed` | `validation:<round>:<label>` |
| 0 eligible | `winner.rejected{no_eligible}` | `winner.rejected:<round>` |
| judge | U31 `judging::start`, then `judging::poll` each tick | U31 keys |
| cleanup | U27 `RoundCleanup::run` | U27 keys |

- The execution id is minted at planning and travels on `candidate.planned`;
  the spawn uses it as `MintedIds.execution`. `find_by_idempotency` is not
  needed: the coordinator reads the store once per tick and indexes it by
  `Execution::idempotency_key()`.
- A record that a killed coordinator started is adopted: with a pane it
  gets its `candidate.spawned`; `Planned` without a pane becomes
  `LaunchFailed{Split, "abandoned..."}` and `candidate.failed{crashed}`.
- Spawn: `SpawnRequest::worker(teammate, rendered task)`, kind `Candidate`,
  `workdir` = worktree, `report_to` None, `pinned`, the planned effort,
  `from_pane` = the dataset workspace's root pane, tiling Disabled (the
  tile hook is a no-op), role `candidate-<label>`. The service runs
  `<ctx.bins.current_exe> worker <role>`, so `run.rs` passes a context
  whose `current_exe` is `horch_exe`.
- Waves: at most `safe_n` live candidates; a free slot starts the next one.
- Observation (`observe::classify`): Done → completed; `Failed(f)` → failed
  f; `LaunchFailed` → crashed; live with the pane gone → the record is
  read again (`horch done` marks it before it closes the pane), then
  PaneVanished; deadline from the record's `created_at` → record first,
  then pane close, then the event. Leftover panes of ended agents are closed.
- Budget: `UsageMeter` prices every candidate session from its transcript
  (`usage::read_session`, the `horch cost` readers). `committed` is 0
  (`SPEC-RESOLVED(Spec B §budget)`: no per-candidate spend estimate). Cancel (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3/§4.11, ai_docs/reports/finish/spec-b-preflight.md)
  closes panes and records `cancelled{budget}`; candidates not started are
  `cancelled{budget}` too. Data stays.
- Disk: `runtime::machine::probe` on the worktree root's disk against
  `caps.disk_headroom_bytes`. Under the floor, unstarted candidates are
  `cancelled{"disk: <free> bytes free, below the <floor> byte floor"}`.
- The meter and the probe run on the first tick and then every 5 s
  (`CHECK_EVERY`); the probe also runs before each launch.
- Freeze and validation run only in VALIDATING (the table allows
  `candidate.frozen` nowhere else). A usage record is written at freeze.
- Exit codes of `run`/`resume`: 0 decided (complete with a winner, or a
  promotion requested), 3 rejected and the budget ended a candidate (or no
  candidate could be planned), 4 preflight, 5 NEEDS_INTERVENTION, 6
  rejected. A fault point returns `FaultFired`; the binary exits 86.

## Decisions

- **Eligible** = the candidate completed AND every gate passed. A failed
  candidate is frozen and validated (kept in the round) but never eligible.
  With 0 gates a completed candidate is eligible, `mechanical_score` 0.0.
- **CARGO_TARGET_DIR**: not moved. `validator.rs` is not my file and
  `with_env` is applied before the worktree-local value. The coordinator
  freezes once per candidate, before validation, so `target/` is never
  committed. B5 revalidates in its own temp worktree.
- **Judge**: no stub. `start`, then `poll(env, round, Utc::now())` each
  tick, `DetachedLauncher(ctx)`, `DEFAULT_STALE_AFTER`, timeout from
  `config.judge.timeout_s`, the `judge` teammate with `config.judge.model`
  and `effort`. A bundle `FsxError::Conflict` records
  `round.needs_intervention{source: operator}` (key
  `needs_intervention:<round>`); `source: judge` is refused before a
  judgment. Other errors stop the coordinator; `resume` polls again.
- **Resume**: no `experiment.created` or no preflight report → preflight
  runs again from `run.json` (orchestrator decision). PLANNED with no round
  → a new round is planned from the manifest config. A round in PLANNED →
  planned again with its own id (missing `candidate.planned` only).
  Otherwise `drive`.
- **run.json** holds the run arguments and the task **redacted** (SEC-01).
  A resumed round briefs candidates with the redacted text.
- **Dataset workspace**: `experiments/<exp>/workspace.json` keeps its id
  and root pane; a resume reuses it while the root pane exists. The
  coordinator closes it when `run` ends. `watch` prints the status every
  3 s, discards stdin, and exits when every round is past VALIDATING.
- **SEC-03**: `artifacts/<round>/usage/<label>.json` holds tokens, cost,
  `transcript_ref` (path) and `transcript_digest`. A copy goes to
  `artifacts/<round>/transcripts/` only with `retain_transcripts`.

## Tests added: 21

- Core `tests/coordinator.rs`: `arc_24_candidates_are_ordinary_executions`.
- E2E `tests/dataset.rs`: `cmp_04_n_worktrees_same_base_modify_same_file`,
  `cmp_05_e2e_candidates_in_dataset_workspace`, `cmp_07_done`,
  `cmp_07_pane_vanished`, `cmp_07_agent_exit`, `cmp_07_timeout`,
  `cmp_07_candidate_crash_kept_in_round`, `cmp_10_hard_budget_cancels_and_retains`,
  `cmp_11_disk_pressure_stops_new_work`, `cmp_13_crash_every_boundary`
  (6 B3 points, 3 B4 coordinator points with `resume`; the 2 judge-job
  points without one), `cmp_13_rebuild_after_power_loss`,
  `cmp_14_all_candidates_fail_round_rejected`, `cmp_15_candidate_task_carries_rules`,
  `mea_10_every_spawn_has_terminal_event`, `sec_03_no_transcript_copies_by_default`,
  `sec_08_e2e_no_api_key_in_any_child`, `jdg_e2e_round_decided` (also checks
  `sessions` hiding and the `--resume` refusal).
- Unit: 2 in `observe.rs`.
- Changed: `pre_07_probe_no_secret_persisted` sets
  `HORCH_FAULT=abort-after-preflight` and accepts exit 86. A passed
  preflight now starts the round; without `exec` panes the round would wait
  for the 1 h deadline.

## Gotchas

- fake-herdr calls race on `state.json`. A `pane get` from a starting
  worker lost its pane to a concurrent split, the worker died before it
  registered, and the record stayed `starting` with a live pane. The fake
  now locks the state (`state.lock/`) and kills process groups only after
  it saved. The same kernel gap exists for real: a worker that dies before
  `set_running` is caught only by the deadline.
- Claude's project dir name is the slugged cwd; a worktree under a temp dir
  gives more than 255 bytes. The fake truncates; horch searches every
  project dir for `<session>.jsonl`.
- Labels follow the slot order: in the pair tests A is codex-sol and B is
  sonnet. `opencode-pickle` is never planned with the all-ok quota fixture.
- The fake judge picks the first bundle label; it now ranks eligible labels
  first, or a round with an ineligible first label is rejected.
- Judge bundle dirs are 0500. The e2e helper `assert_clean` makes them 0700
  so the harness can delete its temp dir.
- `horch sessions --json` also hides candidate and judge records (use
  `--all`). `horch ledger list --json` shows every record.
- The judge-job fault `abort-in-judge-job-before-output` kills both
  attempts (the job inherits `HORCH_FAULT`), so the round ends in
  NEEDS_INTERVENTION (exit 5).

## SPEC-RESOLVED (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3-§11, ai_docs/reports/finish/spec-b-preflight.md)

- `SPEC-RESOLVED(Spec B §budget)`: the expected spend of a running candidate (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3/§4.11, ai_docs/reports/finish/spec-b-preflight.md)
  (`committed` is 0), in `coordinator.rs`.

## Outside my scope (not fixed)

- 3 test fixture literals of `Brief` needed the new field
  (`messaging/mailbox.rs`, `execution/lifecycle.rs` tests,
  `tests/execution_plan.rs`): one line each, mechanical.
- `CommandValidator` gets no `HORCH_FAULT` points (`Faults` has no
  accessor for its set), so `fail-gate:<name>` does not reach the gates
  from `run`.
- A worker that fails before `set_running` leaves a `Starting` record with
  a live pane (see Gotchas); only the deadline ends it.
