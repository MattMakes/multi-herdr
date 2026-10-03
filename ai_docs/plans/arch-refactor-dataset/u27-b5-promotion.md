# U27 b5-promotion: GitPromotionEngine, receipts, cleanup, rollback; path-safe dataset ids

Unit slug: `b5-promotion`. Branch: `ard/b5-promotion`. Phase: B5 (engine part).
Requirements: PRO-01, PRO-03, PRO-04, PRO-05, PRO-06, PRO-07, PRO-02 (unit
part `pro_02_revalidation_failure_rejected`). (PRO-08 and
`pro_02_e2e_only_valid_candidate_promotable` need the binary; a later unit.)

## GOAL

A frozen winning candidate can be promoted into a target branch by a
deterministic engine that never double-promotes, never touches a dirty
checkout, never resolves a conflict automatically, revalidates the
integrated commit, writes a durable receipt before any cleanup, and can be
rolled back by compare-and-swap. Dataset paths refuse ids that are not plain
path components.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: OD5, §3 "B5" (all of it), §4 PRO rows, the
  §5 crash points `abort-after-promotion-started`, `abort-after-update-ref`,
  `abort-after-receipt`, `abort-during-cleanup:<n>`, §4 Spec B failure matrix.
- Design doc `ai_docs/designs/2026-10-02-dataset-competition-design.md` §4.8
  (`BranchRef`, `PromotionStrategy`, `PublishMode`, `PromotionPlan`,
  `PromotionReceipt`, `PromotionResult`, `PromotionEngine`,
  `GitPromotionEngine`) and §5.2 (the promote path table). The design wins
  where it differs from this plan.
- Merged code and reports (`ai_docs/reports/arch-refactor-dataset/`):
  `b2-vcs.md` (`GitClient`: `rev_parse`, `rev_list`, `is_ancestor`,
  `update_ref_cas` returns false on mismatch, `cherry_pick` →
  Clean/Conflict and aborts, `merge_ff_only`, `branch_checkout_location`,
  `worktree_add/remove`; `WorktreeManager`, `FrozenCandidate`;
  `CommandValidator` with `faults: BTreeSet<String>` and `with_env`; freeze
  runs `git add -A`, so keep `CARGO_TARGET_DIR` outside the candidate tree
  when you revalidate), `b1-measure.md` (events `PromotionStarted`
  (`strategy` is a `serde_json::Value` placeholder: type it now with
  `PromotionStrategy`), `PromotionCompleted`, `PromotionConflicted`,
  `PromotionRolledBack`, `WorktreeCleanupFailed`; `JsonlRecorder`;
  `DatasetPaths`), `b3-planner.md` (`competition::state::transition`,
  `RoundEvent`; it allows `COMPLETE → DECIDED` on an operator promote, and
  says B5 must add an `operator.promote` event kind), `b4-evaluation.md`
  (`RejectReason`), `b1-primitives.md` (`fsx::create_immutable`),
  `a2-runtime.md` (`runtime::fault::Faults` has `has()`; add the indexed
  form `abort-during-cleanup:<n>` support locally if `indexed` is absent).
- Security finding from U24: `RoundId` and `ExperimentId` accept `/` and
  `..`, and `DatasetPaths` joins them into paths. Fix it here: every
  `DatasetPaths` accessor that takes an id validates it as one plain path
  component (non-empty, no `/`, `\`, `..`, NUL, not `.`) and returns an
  error otherwise. Do not change the id types' validation (old ledgers carry
  legacy ids).
- Parallel units: U18 `a4-harness`, U23 `b4-judge-input`, U25
  `a6c-presentation`, and the coming A6 service unit. You own only the files
  below.

## FILES

own:
- `crates/horch-core/src/competition/{promotion,cleanup}.rs` (new), `competition/mod.rs` (your lines)
- `crates/horch-core/src/competition/state.rs` (only to add the
  `operator.promote` event and the `REVALIDATING` transitions if absent)
- `crates/horch-core/src/measure/event.rs` (type `PromotionStarted.strategy`;
  add `OperatorPromote { target }` as `operator.promote` and
  `PromotionRolledBack` fields if needed)
- `crates/horch-core/src/measure/paths.rs` (path-component validation)
- `crates/horch-core/src/measure/projection.rs` (only to apply the new event)
- `crates/horch-core/tests/promotion.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/b5-promotion.md`

do not touch: the WorkerRun and export goldens (if a change would alter
them, stop and send `QUESTION:`), every other file.

## STEPS

1. Create the worktree (conventions §2).
2. `measure/paths.rs`: the path-component validation, with tests
   `dataset_paths_reject_traversal_ids` (`../x`, `a/b`, `.`, `..`, empty).
3. `competition/promotion.rs`: types per design §4.8 and
   `GitPromotionEngine::promote(candidate, target, plan)`:
   1. Stale check: the worktree HEAD and the candidate branch both equal
      `candidate.head_sha`; else `Rejected{StaleJudgment}` and
      `winner.rejected{stale_judgment}`.
   2. `dest_before` = `rev_parse(target)`.
   3. Integrate: if `dest_before == base_sha` (or `is_ancestor(dest_before, base)`
      per design), the planned result is the candidate head (fast-forward);
      else create a temp integration worktree under
      `plan.integration_root` at `dest_before` and `cherry_pick(base..head)`
      with the deterministic identity and dates. `Conflict{paths}` →
      `promotion.conflicted{paths}` → `NeedsIntervention`; keep every
      worktree; remove only the temp integration worktree.
   4. Revalidate the integrated commit with the validator (its own temp
      worktree at the integrated SHA; `CARGO_TARGET_DIR` outside it). Any
      failure → `Rejected{RevalidationFailed}`.
   5. Publish: `branch_checkout_location(target)`:
      not checked out → `update_ref_cas(target, planned_after, dest_before)`;
      checked out and clean → `merge_ff_only(planned_after)` in that
      checkout; checked out and dirty → `NeedsIntervention` (touch nothing).
      CAS lost → `NeedsIntervention`.
   6. Order of records: `promotion.started{dest_before, planned_after, strategy}`
      before publish → publish → receipt `promotions/<round>.json` via
      `create_immutable` → `promotion.completed{receipt_digest, dest_after}`.
   Restart rule (`resume_promotion`): read the last `promotion.started`;
   if the target ref == `planned_after` → write the receipt (idempotent)
   and complete; if == `dest_before` → retry the publish; else →
   `NeedsIntervention`. Fault points: `abort-after-promotion-started`,
   `abort-after-update-ref`, `abort-after-receipt`.
4. `rollback(round)`: requires a receipt; CAS the target from `dest_after`
   back to `dest_before`; emit `promotion.rolled_back`; refuse if the ref
   moved since (NeedsIntervention, no change).
5. `competition/cleanup.rs`: remove candidate worktrees only after a durable
   receipt exists (or at round end when not promoting); keep branches unless
   `prune_branches`; a failure emits `worktree.cleanup_failed` and continues;
   NEEDS_INTERVENTION rounds are skipped unless the operator forces cleanup.
   Fault `abort-during-cleanup:<n>` stops after the n-th removal.
6. Tests in `crates/horch-core/tests/promotion.rs` (temp repos, real git,
   `HORCH_REQUIRE_GIT` rule, a fake `Validator` where the gate result is
   scripted):
   - `pro_01_stale_judgment_cannot_promote` (a commit added after freeze).
   - `pro_02_revalidation_failure_rejected`.
   - `pro_03_ff`, `pro_03_cherry_pick_when_target_moved`,
     `pro_03_checked_out_dirty_needs_intervention` (the dirty checkout's
     files and HEAD are byte-identical afterwards).
   - `pro_04_conflict_needs_intervention_preserves_worktrees`.
   - `pro_05_receipt_fields`, `pro_05_cleanup_after_receipt_only`.
   - `pro_06_crash_after_update_ref_resumes_once` (fault
     `abort-after-update-ref`, then `resume_promotion`: exactly 1 receipt,
     the ref advanced once, `promotion.completed` once).
   - `pro_07_cleanup_failure_recorded_history_intact` (make one worktree
     dir undeletable with permissions, `cfg(unix)`; the event is recorded,
     other worktrees are removed, branches remain).
   - `pro_rollback_cas`.
7. Gate after each step. Commits: `B5: Validate dataset path components`,
   `B5: Add the git promotion engine`, `B5: Add promotion restart and rollback`,
   `B5: Add round cleanup`, `B5: Add PRO tests`.
8. Write and commit the report. Follow conventions §6.

## DONE WHEN

- The named tests pass with `HORCH_REQUIRE_GIT=1`. `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: engine API for the CLI unit, restart rule, gotchas.
