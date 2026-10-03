# U37 dataset-gaps: unique experiment branch prefix; preflight checks --promote-to

Unit slug: `dataset-gaps`. Branch: `ard/dataset-gaps`. Phases: B2/B3/B5 follow-up.
Requirements touched: PRE-01, PRO-08 (no new IDs).

## GOAL

Two experiments started in the same minute in one repo do not collide on
candidate branch names, and `run --promote-to <B>` refuses in preflight
(exit 4, no worktree, no model) when B cannot be a promotion target.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Gaps from `ai_docs/reports/arch-refactor-dataset/b5-cli.md` "Outside my scope":
  1. `exp8` is the first 8 hex digits of a UUIDv7 experiment id: those are
     the millisecond timestamp, so 2 experiments within about 65 s share
     it and their branches `mh/exp/<exp8>/r<n>/<L>` collide; PRE-01 refuses
     the second.
  2. `run --promote-to <missing branch>` passes preflight; the engine fails
     in REVALIDATING (exit 1) and the round stays there.
- Code: `exp8` is computed in 3 places: `crates/horch/src/dataset/run.rs`
  (about line 555), `crates/horch-core/src/competition/coordinator.rs`
  (about lines 275 and 344: branch spec and the dataset workspace label).
  `vcs/worktree.rs` validates it.
- Fix 1: 1 function, `ExperimentId::short()` (or a free function in
  `competition/`) that returns 8 hex digits from the random tail of the id
  (for example the last 8 hex digits). All 3 places call it. Keep the field
  name `exp8` and the branch format. Old experiments keep their recorded
  branch names: never recompute a branch for an existing experiment from
  the id if it is recorded in events (check how `resume` gets branch names;
  if it recomputes, keep the old rule for experiments whose
  `worktree.created` events show the old prefix, or read the branch from
  the event).
- Fix 2: a preflight check (reuse the PRE-01 git check family; no new
  requirement ID): with `--promote-to B`, B must exist as a local branch
  (`rev_parse refs/heads/B`) and must not be a candidate namespace branch
  (`mh/exp/...`). Fail → exit 4 with a clear message.
- No other worker is active except, possibly, U33 later. You are alone now.

## FILES

own:
- `crates/horch/src/dataset/run.rs` (exp8 only), `crates/horch/src/dataset/preflight.rs` (the target check)
- `crates/horch-core/src/competition/coordinator.rs` (exp8 only)
- `crates/horch-core/src/competition/preflight.rs` (only if the pure evaluate needs the new fact)
- `crates/horch-core/src/ids.rs` (only the helper, if you put it there)
- `crates/horch-e2e/tests/dataset.rs`, `crates/horch-e2e/tests/promotion.rs` (new tests; remove the branch-deleting workaround U32 added if it is no longer needed)
- `ai_docs/reports/arch-refactor-dataset/dataset-gaps.md`

do not touch: oracle and golden data (if a golden holds an exp8 value,
stop and send `QUESTION:`).

## STEPS

1. Create the worktree (conventions §2).
2. Fix 1 and test `pre_01_two_experiments_same_minute_no_branch_collision`
   (2 `run`s back to back in 1 repo; both pass PRE-01, branch sets disjoint).
3. Fix 2 and test `pro_08_promote_to_missing_branch_refused_in_preflight`
   (exit 4, `experiment.aborted`, 1 worktree in `git worktree list`, no
   agent launch in the fake logs).
4. Gate. Commits `B3: Use the random part of the experiment id in branch names`,
   `B5: Check the promotion target in preflight`. Write and commit the
   report. Follow conventions §6.

## DONE WHEN

- Both tests pass with `HORCH_REQUIRE_GIT=1`; `just gate` is green.

## REPORT

- `horch done` summary: the helper, how resume gets branch names, the new check.
