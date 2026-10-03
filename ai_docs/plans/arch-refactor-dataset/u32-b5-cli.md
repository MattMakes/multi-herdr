# U32 b5-cli: promote, rollback and cleanup commands; --promote-to; candidate and judge cost

Unit slug: `b5-cli`. Branch: `ard/b5-cli`. Phases: B5 (CLI part), B6 (EXP-07).
Requirements: PRO-08, PRO-02 (e2e part), CMP-13 (B5 points in the e2e
suite), EXP-07.

## GOAL

Promotion is opt-in and works end to end from the dataset binary: `run`
without `--promote-to` collects only (DECIDED → CLEANUP → COMPLETE with
`winner.selected{promotion: not_requested}`); `run --promote-to B` and a
later `promote <round> --to B` run the merged `GitPromotionEngine`;
`rollback <round>` and `cleanup <round>` work; and `horch cost` /
telemetry count candidate and judge executions.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: OD5, §3 "B5" (CLI line), §4 PRO-02, PRO-08,
  EXP-07 rows, §5 fault points `abort-after-promotion-started`,
  `abort-after-update-ref`, `abort-after-receipt`, `abort-during-cleanup:<n>`,
  §5 LA checks that use promotion.
- Design doc `ai_docs/designs/2026-10-02-dataset-competition-design.md` §5.1,
  §5.2 (both paths; NEEDS_INTERVENTION → CLEANUP only on an operator
  `cleanup`), §7.2 (at most 1 receipt; the ref advanced at most once).
- Merged reports (`ai_docs/reports/arch-refactor-dataset/`):
  `b5-promotion.md` (the engine API, `resume_promotion`, `rollback`,
  `cleanup`, the `operator.promote` event), `b3-coordinator.md` (the loop,
  `resume`, the DECIDED handling), `b4-judge-job.md` (`JudgingStatus`, the
  fake-claude judge mode), `b2-binary.md` (the hidden `promote`,
  `rollback`, `cleanup` placeholders; exit codes 0, 3, 4, 5, 6),
  `b6-export.md`, `e2e-fakes.md`, the telemetry design for `horch cost`.
- Gotchas carried forward: check the round state before every operator
  command and refuse with a clear message and the right exit code; a
  promotion from NEEDS_INTERVENTION is allowed only when the round has a
  DECIDED winner; candidate worktree dirs can be 0500 after freeze or bundle
  build: restore write permission before removal.

- Engine decisions from U27 (read its report): `PromotionPlan` has
  `experiment` and `attempt` (the CLI sets attempt 1, plus 1 per
  `operator.promote`); the caller moves the round to REVALIDATING first
  (`winner.selected{requested}` or `operator.promote`); a fault point
  returns the error `FaultFired` and the binary must abort the process on
  it; the stale check uses the branch alone when the worktree is gone, so
  `promote <round>` works after COMPLETE; rollback refuses a checked-out
  target; integration and revalidation share 1 temp worktree on branch
  `mh/promote/<round>/a<attempt>`.

- From U30 (`b3-coordinator.md`): `run` now drives the whole round and
  waits for the judge; `resume <exp>` exists. Eligible = completed and
  every gate passed. `run.json` holds the redacted run arguments (store
  `--promote-to` there so `resume` keeps it). Labels follow slot order (in
  the pair fixtures A is codex-sol, B is sonnet). The fake judge ranks
  eligible labels first. Judge bundle dirs are 0500 (`assert_clean` in the
  e2e lib makes them 0700). The judge-job fault kills both attempts (the
  job inherits `HORCH_FAULT`). `horch sessions --json` hides candidate and
  judge records; use `--all` or `horch ledger list --json`. Reuse the
  e2e helpers U30 added to `crates/horch-e2e/src/lib.rs`.

- Gap from U30: `CommandValidator` gets no `HORCH_FAULT` points, so
  `fail-gate:<name>` does not reach the gates from `run`. Fix it here (you
  need it for `pro_02_e2e_only_valid_candidate_promotable`): add a
  read-only accessor on `runtime::fault::Faults` that returns the set (or
  the `fail-gate:` names), and pass it where `run`/the coordinator builds
  the `CommandValidator`. You own those lines in `runtime/fault.rs` and the
  validator construction in `coordinator.rs`/`run.rs`.

## FILES

own:
- `crates/horch/src/dataset/{promote,rollback,cleanup}.rs` (new), and their
  arms: the arms live in `crates/horch/src/dataset/cli.rs` (the clap `Placeholder` variant: give it real args) and `crates/horch/src/dataset/mod.rs` (the dispatch `not_implemented(...)` line). Only the `Promote`, `Rollback`, `Cleanup` lines.
- `crates/horch/src/dataset/run.rs` (only the `--promote-to` handling after
  DECIDED)
- `crates/horch-core/src/competition/coordinator.rs` (only the DECIDED
  branch: promotion requested → engine; else cleanup)
- `crates/horch-e2e/tests/promotion.rs` (new), `crates/horch-e2e/tests/dataset.rs` (extend the crash suite list only)
- `crates/horch-e2e/tests/usage_dataset.rs` (new, EXP-07)
- `ai_docs/reports/arch-refactor-dataset/b5-cli.md`

do not touch: `competition/{promotion,cleanup}.rs` (call them; ask if the API
lacks something), `evaluation/**`, telemetry code (EXP-07 is a test of
existing behavior; if it fails, send `QUESTION:` with the cause before
editing telemetry).

## STEPS

1. Create the worktree (conventions §2).
2. DECIDED handling in the coordinator: no `--promote-to` → CLEANUP →
   COMPLETE; with `--promote-to B` → REVALIDATING → engine → PROMOTED,
   REJECTED or NEEDS_INTERVENTION per §5.2; map the round's final state to
   the exit code (DECIDED/PROMOTED/COMPLETE 0, NEEDS_INTERVENTION 5,
   REJECTED 6).
3. `promote <round> --to B`: allowed from COMPLETE or NEEDS_INTERVENTION
   with a DECIDED winner (records `operator.promote{target}` first); the
   candidate worktree must still exist (else a clear refusal: the round was
   cleaned up; promotion needs the frozen worktree or its branch — use the
   branch when the engine allows it, document which). `resume` of a
   PROMOTING round calls `resume_promotion`.
4. `rollback <round>`: the engine's CAS rollback; refuse when the ref moved
   (exit 5).
5. `cleanup <round> [--force] [--prune-branches]`: the engine cleanup;
   from NEEDS_INTERVENTION only with `--force`.
6. Tests (e2e, fakes, `Harness::with_git()`, `HORCH_REQUIRE_GIT=1`):
   - `pro_08_default_collects_only` (2 candidates, a valid judgment, no
     `--promote-to`: the target ref is unchanged, the round is COMPLETE,
     `winner.selected{promotion: not_requested}`, branches kept).
   - `pro_08_promote_to_and_promote_cmd` (`run --promote-to B` advances B
     once with a receipt; a second round collected first and then
     `promote <round> --to B` does the same).
   - `pro_02_e2e_only_valid_candidate_promotable` (the gate fails on one
     candidate: it is not eligible; the judge picks the other; promotion
     revalidates and publishes only that one).
   - `pro_rollback_e2e` and `pro_cleanup_needs_force_from_intervention`.
   - Crash suite: add the 4 B5 points to `cmp_13_crash_every_boundary`; after
     each, `resume`: at most 1 receipt, the ref advanced at most once,
     projection equals `rebuild`.
   - `exp_07_candidate_and_judge_in_usage` (`crates/horch-e2e/tests/usage_dataset.rs`):
     after a fake round whose fakes write usage, the telemetry snapshot (or
     `horch cost --json`) lists every candidate session and the judge
     session with their cost.
7. Gate after each step. Commits: `B5: Promote after DECIDED when requested`,
   `B5: Add promote, rollback and cleanup commands`, `B5: Add PRO-08 and PRO-02 e2e`,
   `B5: Extend the crash suite`, `B6: Add EXP-07 test`.
8. Write and commit the report. Follow conventions §6.

## DONE WHEN

- Every named test passes with `HORCH_REQUIRE_GIT=1`.
- `check-req-coverage.sh --phase B5` and `--phase B6` exit 0.
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: the operator commands, their refusal rules and exit
  codes, gotchas.
