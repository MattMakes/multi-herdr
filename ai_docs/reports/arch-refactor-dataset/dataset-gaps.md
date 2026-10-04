# U37 dataset-gaps: report

Branch `ard/dataset-gaps`. Phases B3 and B5 follow-up. Requirements PRE-01,
PRO-08 (no new IDs).
`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` is green after every
commit. `check-req-coverage.sh --phase B3` and `--phase B5` exit 0.

## Commits

1. `B3: Use the random part of the experiment id in branch names`
2. `B5: Check the promotion target in preflight`
3. This report.

## Fix 1: the branch prefix (`exp8`)

- `ExperimentId::short()` in `crates/horch-core/src/ids.rs` returns the last
  8 letters and digits of the id. For a UUIDv7 these are 8 random hex
  digits. The first 8 hex digits are the top of the millisecond timestamp,
  so they change about every 65 s only.
- A legacy id that is not a UUID gives its last 8 letters and digits
  (`exp-1` gives `exp1`), so `WorktreeSpec::check` still accepts it.
- The field name `exp8` and the branch format `mh/exp/<exp8>/r<idx>/<label>`
  do not change.
- Callers: `planned_branches` in `crates/horch/src/dataset/run.rs` (the
  PRE-01 namespace check) and the dataset workspace label in the
  coordinator. The old free fn `run::exp8` is removed; it had no other
  caller.

### How resume gets branch names

- The coordinator recomputes a `WorktreeSpec` in 2 places: `provision`
  (candidates with no `worktree.created` yet) and the freeze step.
  `WorktreeManager::freeze` checks that the worktree is on `spec.branch()`.
- So a round started before this change would fail its freeze after an
  upgrade if the prefix were recomputed from the id.
- `exp8_of(spec, view)` in `competition/coordinator.rs` returns the prefix of
  the round's recorded `worktree.created` branches. Only a round with no
  recorded worktree uses `short()`. An old round keeps its old prefix, also
  for the candidates it provisions after a resume.
- `planned_branches` in `run.rs` runs only in `preflight_and_run`, for an
  experiment with no worktrees. The new rule is correct there.

## Fix 2: `--promote-to` in preflight

- `GitFacts` gets `promote_target_problem: Option<String>` (serde default,
  skipped when `None`). `pre_01_git` adds its text to the PRE-01 problems
  and to the measured JSON.
- `dataset::preflight::repo_facts` takes `promote_to: Option<&str>`. The
  target fails when it starts with `mh/exp/` or when
  `rev-parse --verify refs/heads/<B>` finds nothing.
- The run then stops with exit 4 and `experiment.aborted` with
  `failed_checks: ["PRE-01"]`. No worktree is created and no model is
  called.

## Tests

- `ids::tests::pre_01_experiment_short_is_the_random_tail` (unit).
- `pre_01_promote_target_problem_fails` in `crates/horch-core/tests/preflight.rs`.
- `pre_01_two_experiments_same_minute_no_branch_collision` in
  `crates/horch-e2e/tests/dataset.rs`: 2 runs back to back in 1 repo, both
  exit 0, PRE-01 passes twice, 4 branches, each under its own prefix.
- `pro_08_promote_to_missing_branch_refused_in_preflight` in
  `crates/horch-e2e/tests/promotion.rs`: a missing branch and a candidate
  branch both give exit 4, PRE-01 in `experiment.aborted`, 1 worktree, no
  candidate branch, agent fakes called with `--version` only.
- The branch-deleting `next_round` helper in `promotion.rs` is removed. The
  3 tests that used it now call `set_candidates`, and they pass.

## Decisions and gotchas

- The orchestrator approved 1 edit outside the owned files: the field
  `promote_target_problem: None` in the `GitFacts` literal of
  `crates/horch-core/tests/preflight.rs`, plus the new unit test there.
- The e2e test does not assert that both ids share their first 8
  characters. That prefix changes about every 65 s and the assertion would
  flake.
- A round of an old experiment and a later round of the same experiment
  can have different prefixes. The round index keeps their branches apart.
- No golden holds an `exp8` value. No golden and no oracle changed.

## Outside my scope (not fixed)

None found.

## SPEC-TODO

None new.
