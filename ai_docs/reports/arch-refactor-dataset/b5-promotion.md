# U27 b5-promotion: report

Branch `ard/b5-promotion`. Phase B5 (engine part). Requirements PRO-01 to
PRO-07, and PRO-02 for the unit part. The gate is green after every commit.
PRO-08 and `pro_02_e2e_only_valid_candidate_promotable` belong to the CLI
unit.

## What landed

| File | Content |
|---|---|
| `measure/paths.rs` | `BadPathComponent`, `component`, `PathResult`. Every accessor that joins an id, a label or a label policy version returns `Result` |
| `competition/promotion.rs` | `BranchRef`, `PromotionStrategy`, `PublishMode`, `PromotionPlan`, `PromotionReceipt`, `PromotionResult`, `RollbackResult`, `FaultFired`, `PromotionEngine`, `GitPromotionEngine` (`promote`, `resume_promotion`, `rollback`) |
| `competition/cleanup.rs` | `RoundCleanup::run`, `CleanupOptions`, `CleanupOutcome` |
| `measure/event.rs` | `PromotionStarted.strategy` is typed. `publish` and `validation_ids` are new (serde default). New kind `operator.promote {target}` |
| `measure/projection.rs` | `operator.promote`: NEEDS_INTERVENTION or COMPLETE → DECIDED → REVALIDATING. A round without a winner gives an anomaly |
| `tests/promotion.rs` | 15 tests |
| `dataset/export.rs`, `tests/measure.rs`, `tests/dataset_export.rs` | call sites only (the orchestrator approved this) |
| `evaluation/judge_input.rs`, `tests/judge_input.rs` | 1 call site each, after the rebase onto U23 (same `?` / `.unwrap()` fix) |

`competition/state.rs` did not change. Its table already has the
REVALIDATING rows and `operator.promote`.

## API for the CLI unit

```rust
let engine = GitPromotionEngine { git: &git, validator: &command_validator,
                                  recorder: &recorder, paths: &paths, faults: &faults };
let plan = PromotionPlan { experiment, round, judgment_id, identity,
                           integration_root, attempt };
engine.promote(&frozen, &BranchRef { repo, name: "main".into() }, &plan)?
    // Promoted(receipt) | NeedsIntervention { reason } | Rejected { reason }
engine.resume_promotion(&frozen, &target, &plan, &round_view.promotion.started.unwrap())?
engine.rollback(&experiment, &round, &target)?   // RolledBack { restored } | NeedsIntervention
RoundCleanup { git, recorder, paths, faults, repo }.run(&round, &round_view, CleanupOptions { prune_branches, force })?
```

Before `promote`, the caller must bring the round to REVALIDATING. Use
`winner.selected{promotion: requested}` or `operator.promote{target}`.
The engine does not emit these events.

- `plan.attempt` is 1 for the first promotion of a round. Add 1 for each
  `operator.promote`. It keys the attempt's events. A retry after
  NEEDS_INTERVENTION records new events. A restart of the same attempt
  records nothing twice.
- Event keys: `promotion.started:<round>:<attempt>`,
  `promotion.completed:<round>`, `promotion.conflicted:<round>:<attempt>`,
  `promotion.rejected:<round>:<attempt>` (kind `winner.rejected`),
  `round.needs_intervention:promotion:<round>:<attempt>`,
  `promotion.rolled_back:<round>`, `round.cleanup_started:<round>:<FROM>`,
  `worktree.cleanup_failed:<round>:<FROM>:<label>`,
  `round.completed:<round>:<FROM>`.
- `rollback` emits with `Actor::Operator`. The other events use
  `Actor::Coordinator`.

## Engine rules

- Stale check (PRO-01). `refs/heads/<candidate.branch>` must equal
  `head_sha`. If the worktree still exists, its HEAD must also equal
  `head_sha`. A round that was cleaned up has no worktree, so the branch
  alone decides. This lets `promote <round>` run after COMPLETE.
- Strategy. If `is_ancestor(dest_before, base_sha)`, the strategy is
  fast-forward and `planned_after` is `head_sha`. Otherwise the engine
  cherry-picks `base..head` onto `dest_before` with `plan.identity`.
- One temp worktree `<integration_root>/<round>-a<attempt>` on the temp
  branch `mh/promote/<round>/a<attempt>`. It integrates, then the validator
  runs in it at `planned_after`. Then it is removed with `--force`. The
  temp branch is deleted when the attempt ends. It stays only after a
  crash; the next attempt or the resume deletes it.
- Publish. If the target is not checked out, the engine uses
  `update_ref_cas`. If it is checked out and clean, the engine checks the
  branch, HEAD == `dest_before` and a clean status, then runs
  `merge --ff-only`. If it is dirty, the engine records
  `round.needs_intervention` and touches nothing. A lost CAS gives
  NEEDS_INTERVENTION.
- Record order: `promotion.started` → publish → receipt
  (`create_immutable`, 0600, pretty JSON + `\n`) → `promotion.completed`
  (`receipt_digest` = sha256 of the receipt file bytes).
- `source_shas` is `rev-list base..head`, oldest first.
- A round has at most 1 receipt. `promote` refuses a round that already
  has one.
- Restart rule (`resume_promotion`, PRO-06). If the target is at
  `planned_after`, the engine writes the receipt and records completed.
  An existing receipt is kept and is not rewritten. If the target is at
  `dest_before`, the engine runs the publish again. In any other case it
  records NEEDS_INTERVENTION.
- Rollback. It needs the receipt. If the target is at `dest_after`, it
  runs a CAS to `dest_before` and records `promotion.rolled_back`. If the
  target is already at `dest_before`, it records the event and changes
  nothing; a second rollback is a no-op. If the target moved, or a
  worktree has it checked out, the result is NeedsIntervention and
  nothing changes. Rollback records no `round.needs_intervention`,
  because COMPLETE has no such row.
- Faults: `abort-after-promotion-started`, `abort-after-update-ref`,
  `abort-after-receipt` and `abort-during-cleanup:<n>`. Each one returns
  the error `FaultFired`, as the marketplace installer fault does. Nothing
  after the fault point runs. The binary decides whether to exit or abort.
  `Faults` has no indexed form, so cleanup checks
  `has("abort-during-cleanup:<n>")`.

## Cleanup rules

- DECIDED → final outcome `winner`. REJECTED → `rejected`. PROMOTED →
  `promoted`, and only when `promotions/<round>.json` exists.
  NEEDS_INTERVENTION → only with `force`. Any other state → `Skipped`.
  CLEANUP (a restart) continues from `cleaned_from` and does not emit
  `round.cleanup_started` again.
- Cleanup removes only worktrees that `git worktree list` shows. A failed
  removal records `worktree.cleanup_failed` and cleanup continues.
  `prune_branches` deletes a branch only after its worktree is removed.

## Decisions and deviations

- The design wins over the plan. `PromotionPlan` has no `gates` because the
  `Validator` holds them. It adds `experiment` (events need it) and
  `attempt`.
- Revalidation and integration share 1 temp worktree. The plan says "its
  own temp worktree". The commit is the same, and 1 worktree has fewer
  failure paths.
- `CommandValidator` sets `CARGO_TARGET_DIR=<worktree>/target`. In this
  engine that is the temp worktree, which is force-removed. The candidate
  tree never gets a `target/`.
- The plan asks for separate commits "Add the git promotion engine" and
  "Add promotion restart and rollback". They are 1 commit
  (`B5: Add the git promotion engine`), because they share one file and
  its helpers.
- Rollback refuses a checked-out target. `GitClient` has no
  `reset --keep`, and `update-ref` under a checkout leaves its files
  behind the branch.

## Gotchas

- `DatasetPaths` accessors now return `Result`. Unit b2-binary uses them
  and must add `?` when it rebases.
- `measure/event.rs` and `measure/projection.rs` must not contain the words
  in `cmp_02_domain_has_no_adapter_imports`. This includes "Command", even
  in a comment.
- A merge ff-only publish has no real CAS. The engine checks the checkout
  just before the merge, but a race in that window is possible.
- The receipt's `publish` after a restart is the mode that the
  `promotion.started` event recorded.
- PRO-07 uses mode 0555 on the worktree dir. `git worktree remove --force`
  then fails, and the other worktrees are still removed.

## SPEC-RESOLVED (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3-§11, ai_docs/reports/finish/spec-b-preflight.md)

- None new. The `operator.promote` kind sits under the existing
  `SPEC-RESOLVED(Spec B event list)` on `EventKind`. (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §4.1/§4.3/§5, ai_docs/reports/finish/spec-b-events.md)

## Tests

`dataset_paths_reject_traversal_ids`, `pro_01_stale_judgment_cannot_promote`,
`pro_02_revalidation_failure_rejected`, `pro_03_ff`,
`pro_03_cherry_pick_when_target_moved`,
`pro_03_checked_out_dirty_needs_intervention`,
`pro_04_conflict_needs_intervention_preserves_worktrees`,
`pro_05_receipt_fields`, `pro_05_cleanup_after_receipt_only`,
`pro_06_crash_after_update_ref_resumes_once`,
`pro_06_restart_after_each_fault_point`,
`pro_07_cleanup_failure_recorded_history_intact`, `pro_rollback_cas`,
`cleanup_fault_stops_after_the_nth_removal_and_resumes`,
`operator_promote_reenters_revalidation`.

`./scripts/check-req-coverage.sh --phase B5` reports PRO-01 to PRO-07 as
ok. PRO-08 is missing; it belongs to the CLI unit.

## Outside my scope (not fixed)

- `tests/execution_store.rs` has the unused import `KIND_ORCHESTRATOR`,
  which gives a build warning. It was there before this unit.
- `runtime::fault::Faults` has no `indexed()`. The A6 note asks for it.
