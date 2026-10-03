# U30 b3-coordinator: the competition round, end to end through the kernel

Unit slug: `b3-coordinator`. Branch: `ard/b3-coordinator`. Phase: B3.
Requirements: CMP-04 (e2e part), CMP-05, CMP-07, CMP-09 (wiring), CMP-10,
CMP-11, CMP-13 (B3 points), CMP-14, CMP-15, MEA-10, SEC-03, SEC-08
(candidates), ARC-24.

## GOAL

`multi-herdr-dataset run` executes a full competition round up to
JUDGING_BACKGROUND (or REJECTED): it creates N worktrees from one base SHA in
a dedicated herdr workspace, spawns candidates in waves as ordinary
executions through the kernel's `ExecutionService`, observes them to a
terminal state, enforces deadline, budget and disk limits, freezes and
validates each candidate, and records every step as an idempotent event so a
killed coordinator resumes without duplicates. Judging is a stub that the B4
job unit (U31) fills; with judging stubbed, a round with ≥ 1 eligible candidate
stops in JUDGING_BACKGROUND and exits 0.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §3 "B3" (all of it, especially items 2, 3, 5,
  6), §4 CMP, MEA-10, SEC-03, SEC-08, ARC-24 rows, the failure matrix, §5
  crash points and resume invariants, §6 risks 2, 3, 6.
- Design doc `ai_docs/designs/2026-10-02-dataset-competition-design.md` §5.1
  (default path), "B3 Competitive worktrees" phase table (kernel additions,
  coordinator, observation table, fakes), §7.1 (fault points
  `abort-after-experiment-created`, `abort-after-preflight`,
  `abort-after-worktree:<n>`, `abort-after-candidate-spawned:<n>`,
  `abort-after-freeze:<L>`, `abort-after-validation:<L>`), §7.2 (resume
  invariants).
- Merged reports to read (`ai_docs/reports/arch-refactor-dataset/`):
  `a6b-service.md` (the `ExecutionService`, `SpawnRequest { workdir, kind, report_to }`,
  idempotency, recovery rule), `b2-binary.md` (the binary, `run` currently
  stops after preflight: replace that), `b3-planner.md` (`plan_round`,
  `BudgetPolicy::check`, the state table; notes: no round-deadline event, so
  emit `candidate.failed{TimedOut}` per candidate; `round.cleanup_started`
  from NEEDS_INTERVENTION only on an operator cleanup), `b2-vcs.md`
  (`WorktreeManager`, `CommandValidator`; freeze runs `git add -A`, so set
  `CARGO_TARGET_DIR` OUTSIDE the worktree, under the dataset dir per label;
  with 0 gates, eligible = true and score 0.0 — decide and document;
  `diff_patch` needs the main repo), `b1-measure.md` (`JsonlRecorder`,
  `StoreOptions::from_faults`, monotonic `occurred_at`, `fold` needs
  `execution_id` on `candidate.planned`/`candidate.spawned`), `b5-promotion.md`
  (validated `DatasetPaths`, cleanup), `a7a-workspace.md` and
  `a7b-messaging.md` (`WorkspaceClient`, `workspace_create(label, focus, cwd)`),
  `a5-routing.md` (`RoutingMode`; add `Pinned`), `e2e-fakes.md` (fake
  scenarios, deterministic ids, `Harness::with_git()`; e2e exec commands need
  absolute paths), `a6c-presentation.md` (`horch sessions` reads the store).
- Kernel additions you own: `RoutingMode::Pinned` (a candidate is never
  substituted after planning), `horch sessions` hides `kind: candidate` and
  `kind: judge` executions unless `--all`, and `horch spawn --resume`
  refuses them.
- Candidate rules travel in the task text through the template
  `teammates/_base/competition-candidate.md` (work only in this dir, commit,
  never push, never message an orchestrator, run `horch done` when
  finished). The golden base prompts must not change (CMP-15).
- Parallel units: U28 `a10-exposure`, U27 `b5-promotion` (if still open),
  and the coming B4 job unit (it fills the judging step and the
  `judge-job` subcommand; leave a clear `judge::schedule` call site).

## FILES

own:
- `crates/horch-core/src/competition/{coordinator,observe}.rs` (new), `competition/judging.rs` (the stub only; U31 replaces its body), `competition/mod.rs` (your lines)
- `crates/horch-core/src/competition/budget.rs` (only to add the live `UsageMeter`)
- `crates/horch-core/src/routing/decision.rs` (only `RoutingMode::Pinned`)
- `crates/horch/src/dataset/run.rs`, `dataset/watch.rs`, `dataset/status.rs` (extend), `crates/horch/src/cmd/ledgercmd.rs` (`--all`), `crates/horch/src/cmd/spawn.rs` (resume refusal line only), `crates/horch/src/main.rs` (`sessions --all` flag only)
- `teammates/_base/competition-candidate.md` (new)
- `crates/horch-e2e/src/bin/fake-{claude,codex,opencode,herdr}.rs` (candidate mode, dataset workspaces)
- `crates/horch-e2e/tests/dataset.rs` (extend)
- `crates/horch-core/tests/coordinator.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/b3-coordinator.md`

do not touch: oracle and golden data, `execution/{service,plan,lifecycle}.rs`
(call them; ask if the API lacks something), `harness/**`, `skills/**`.

## STEPS

1. Create the worktree (conventions §2).
2. Template `teammates/_base/competition-candidate.md` with the rules above
   and placeholders for the task and the worktree path; a renderer in
   `coordinator.rs` fills it. Check: `teammates --check` passes; golden
   prompts unchanged.
3. `competition/coordinator.rs`: a single-threaded tick loop over
   (recorder, git, workspace client, execution service, validator, clock,
   faults). Sequence with events and idempotency keys:
   experiment.created → preflight (reuse B2) → round.created + candidate.planned
   (`plan:<round>:<label>`) → worktrees (`WorktreeManager::create`;
   `worktree.created`, key `worktree:<round>:<label>`) → dedicated workspace
   `multi-herdr-dataset <exp8>` (no focus; root pane runs `watch`, which
   discards stdin; no `orchestrator` registration; tiling Disabled) →
   spawn in waves of `safe_n` through `ExecutionService` with
   `SpawnRequest { kind: Candidate{experiment, round, label}, workdir: worktree, report_to: None, routing: Pinned }`
   (key `spawn:<round>:<label>`; on resume, adopt the existing execution) →
   observe (step 4) → on each terminal candidate: freeze
   (`candidate.frozen`) → validate (`validation.completed`) → when every
   candidate is validated: 0 eligible → `winner.rejected{no_eligible}` →
   REJECTED → cleanup → COMPLETE (keep data); ≥ 1 eligible →
   JUDGING_BACKGROUND → call `competition::judging::start(&env, &round)`
   and, each tick, `competition::judging::poll(&env, &round, now)`
   returning `JudgingStatus { Waiting, Decided(WinnerOutcome), NeedsIntervention }`.
   U31 `b4-judge-job` writes `competition/judging.rs`. You add only a stub
   file with these 2 functions (`start` records nothing and returns Ok;
   `poll` returns Waiting) and a `JudgeEnv` struct holding what the judge
   needs (recorder, paths, git, ctx, clock, faults). `run` exits 0 while
   `poll` says Waiting and the judge stub is in use (B4 replaces this:
   `run` then waits for the job).
   Every fault point in CONTEXT is checked after its event.
4. `competition/observe.rs`: per tick, per live candidate: execution state
   Done → `candidate.completed`; pane gone while live →
   `Failed(PaneVanished)`; agent exit → `Failed(AgentExited)`; deadline
   (`caps.candidate_deadline_s`) → close the pane → `Failed(TimedOut)`;
   budget (`BudgetPolicy` via `UsageMeter`, which polls the telemetry
   readers for each candidate session and converts to µ$) → StopLaunches or
   Cancel (close panes → `Failed(Cancelled{budget})`), keeping all data.
   Disk check each tick (`runtime::machine::probe` free space vs the
   preflight headroom): under the floor → stop new launches and record why.
5. `dataset/run.rs`: replace the B2 "not implemented" exit with the
   coordinator; exit codes 0, 3, 4, 5, 6. Add `resume <exp>` (re-enter the
   loop from the projection). `dataset/watch.rs`: the root pane's command
   (prints round status every few seconds; discards stdin).
6. Kernel additions: `RoutingMode::Pinned`; `horch sessions` hides
   candidate and judge kinds unless `--all`; `horch spawn --resume` refuses
   them with a clear error.
7. Fakes: candidate mode keyed by `$HORCH_FAKE_LOG.candidates.json`
   (per cwd or per label: `{write: {path: content}, commit: bool, exit: done|crash|hang|exit0}`);
   `done` runs `horch done` through the worker flow as a real agent would;
   fake-herdr exempts dataset workspaces from violations, kills on close.
8. Tests (`crates/horch-e2e/tests/dataset.rs` unless noted):
   - `cmp_04_n_worktrees_same_base_modify_same_file` (e2e variant if the
     core one exists, else extend), `cmp_05_e2e_candidates_in_dataset_workspace`
     (cwd of each candidate is its worktree; panes are in the dataset
     workspace; no orchestrator pane registered).
   - `cmp_07_done`, `cmp_07_pane_vanished`, `cmp_07_agent_exit`,
     `cmp_07_timeout`, `cmp_07_candidate_crash_kept_in_round`.
   - `cmp_10_hard_budget_cancels_and_retains`, `cmp_11_disk_pressure_stops_new_work`
     (machine fixture with low disk after the first wave).
   - `cmp_13_crash_every_boundary` (the 6 B3 fault points; after each,
     `resume`: exactly N executions, projection equals `rebuild`),
     `cmp_13_rebuild_after_power_loss` (truncate the last event line).
   - `cmp_14_all_candidates_fail_round_rejected`.
   - `cmp_15_candidate_task_carries_rules` (+ `golden_prompts` unchanged).
   - `mea_10_every_spawn_has_terminal_event`.
   - `sec_03_no_transcript_copies_by_default` (the dataset dir holds
     transcript references and digests, no transcript bytes, unless
     `retain_transcripts`).
   - `sec_08_e2e_no_api_key_in_any_child` (candidates part: with
     `ANTHROPIC_API_KEY=SENTINEL`, no fake's recorded env has it and
     `grep -r SENTINEL` over the state dir finds nothing).
   - `arc_24_candidates_are_ordinary_executions` (core test: candidate
     records live in the normal execution store with `kind: candidate`,
     created through `ExecutionService`; no second registry file exists).
9. Gate after each step. Commits by step. Write and commit the report.
   Follow conventions §6.

## DONE WHEN

- Every named test passes with `HORCH_REQUIRE_GIT=1`.
- `check-req-coverage.sh --phase B3` exits 0 (with earlier B3 units merged).
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: the coordinator loop, idempotency keys, the judge
  call site for the B4 job unit, gotchas.
