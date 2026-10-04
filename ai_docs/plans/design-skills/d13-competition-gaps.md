# D13 competition-gaps: race-free publish into a checked-out branch; honest budget for running candidates

Unit slug: `competition-gaps`. Branch: `ds/competition-gaps`.

## GOAL

Promotion into a checked-out clean branch cannot publish over a commit that
landed between the check and the merge, and the round budget counts the
expected spend of candidates that are still running.

## CONTEXT

- Read first: `00-conventions.md`, `ai_docs/reports/arch-refactor-dataset/b5-promotion.md`
  (gotcha: "A merge ff-only publish has no real CAS"), `b3-coordinator.md`
  (`SPEC-TODO(Spec B §budget)`: committed spend is 0), `b3-planner.md`
  (`BudgetPolicy::check`), `b2-binary.md` (preflight projected cost per
  candidate).
- Gap 1 (`crates/horch-core/src/competition/promotion.rs`, `vcs/git.rs`):
  publish into a checked-out clean branch uses `merge --ff-only` after a
  separate check. Make it a compare-and-swap: for example, verify HEAD ==
  `dest_before` and the tree is clean, run the fast-forward with an explicit
  expected old value (`git merge --ff-only` into a detached check, or
  `update-ref <ref> <new> <old>` followed by a checkout-safe tree update such
  as `git read-tree -m -u <old> <new>` or `git reset --keep <new>` guarded by
  the CAS), then verify HEAD == `planned_after`. On any mismatch:
  NEEDS_INTERVENTION, nothing half-applied. Choose the simplest correct
  sequence, explain why it is atomic enough in the report, and test the race
  (a commit lands on the target between the check and the publish).
- Gap 2 (`competition/budget.rs`, `coordinator.rs`): `committed` spend is 0,
  so a round can overrun the hard budget while candidates run. Policy: for
  each running candidate, committed = max(measured so far, its planned
  projected cost from the round plan); for the judge, its reserve until it
  completes. This matches what preflight PRE-09 promised. Replace the
  `SPEC-TODO(Spec B §budget)` with a comment that states the policy and
  that it is the orchestrator's decision pending the Spec B text. Test:
  a round whose projected spend crosses the hard limit stops new launches
  before measured spend does.

## FILES

own:
- `crates/horch-core/src/competition/{promotion,budget,coordinator}.rs`
  (only these gaps), `crates/horch-core/src/vcs/git.rs` (a new git call if needed)
- `crates/horch-core/tests/promotion.rs`, `crates/horch-core/tests/coordinator.rs`,
  `crates/horch-e2e/tests/dataset.rs` (new tests only)
- `ai_docs/reports/design-skills/competition-gaps.md`

do not touch: event formats (a new field needs serde default and no golden
change), lifecycle, skills, teammates.

## STEPS

0. Create the worktree (conventions §3).
1. Gap 1 with `pro_03_checked_out_publish_is_cas` (temp repos, real git).
   Gate. Commit.
2. Gap 2 with `cmp_10_committed_spend_counts_running_candidates`. Gate. Commit.
3. Report. Follow conventions §7.
