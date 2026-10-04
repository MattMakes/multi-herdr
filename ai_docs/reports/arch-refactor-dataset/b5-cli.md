# U32 b5-cli: report

Branch `ard/b5-cli`. Phases B5 (CLI part) and B6 (EXP-07). Requirements
PRO-08, PRO-02 (e2e), CMP-13 (B5 points), EXP-07.
`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` is green after every
commit. `check-req-coverage.sh --phase B5` and `--phase B6`: every ID ok.

## Commits

1. `B5: Promote after DECIDED when requested`
2. `B5: Add promote, rollback and cleanup commands`
3. `B5: Add PRO-08 and PRO-02 e2e`
4. `B5: Extend the crash suite`
5. `B6: Add EXP-07 test`
6. This report.

## The coordinator after the judge (`competition/coordinator.rs`)

`decide` loops on the round state:

| State | Action |
|---|---|
| DECIDED (no promotion requested), REJECTED, CLEANUP, PROMOTED | `RoundCleanup::run` (a `Skipped` result is an error, so the loop cannot spin) |
| REVALIDATING | `RoundPromoter::advance` → `GitPromotionEngine::promote` |
| PROMOTING | `RoundPromoter::advance` → `resume_promotion` with `promotion.started` |
| COMPLETE | outcome from `final_outcome` |
| NEEDS_INTERVENTION | stop |

`winner.selected{requested}` and `operator.promote` both fold straight to
REVALIDATING, so DECIDED only holds a round that collects.

New public items, shared with the `promote` command:

- `RoundPromoter { git, validator: &dyn Validator, recorder, paths, faults, repo, integration_root }`.
- `promotion_request(events, round, view) -> (target, attempt)`. The last
  `operator.promote` target wins over the `winner.selected` target. The
  attempt is 1, plus 1 per `operator.promote`.
- `frozen_winner(view, at)`: the `FrozenCandidate` from the events.
- `integration_root(worktree_root)` = `<worktree root>/_promote`.
- The cherry-pick identity is the freeze identity (`FREEZE_NAME`,
  `FREEZE_EMAIL`) with the current time.

`runtime::fault::Faults::points()` returns the armed set. `run.rs` passes it
to `CommandValidator`, so `fail-gate:<name>` reaches the gates from `run`
and from promotion's revalidation (the gap from U30).

## Operator commands and exit codes

| Command | Allowed | Refusal |
|---|---|---|
| `promote <round> --to <B>` | COMPLETE, or NEEDS_INTERVENTION, with a winner and no rejection | exit 1: other state, no winner, rejected winner, receipt exists, winner branch pruned or moved, target missing |
| `rollback <round>` | PROMOTED or COMPLETE with `promotion.completed` | exit 1: never promoted or wrong state; exit 5: the target moved or is checked out (nothing changes) |
| `cleanup <round> [--force] [--prune-branches]` | DECIDED, REJECTED, PROMOTED (with receipt), CLEANUP; NEEDS_INTERVENTION with `--force` | exit 5: NEEDS_INTERVENTION without `--force`; exit 1: a live round; COMPLETE: exit 0, no-op |

An unknown round is an error: exit 1, `no such round <id>`.

- `promote` records `operator.promote{target}` (actor `operator`, key
  `operator.promote:<round>:<n>`), then calls `run::resume` on the round's
  experiment. The coordinator promotes, then cleans up after the receipt.
  The exit codes are those of `run`: 0 PROMOTED/COMPLETE, 5
  NEEDS_INTERVENTION, 6 REJECTED, 86 on a fault.
- Promotion after cleanup uses the branch alone (the engine allows it).
  `promote` refuses only when the winner branch is gone (pruned) or moved.
- `cleanup` gives the owner write permission on every directory of each
  candidate worktree first (0500 dirs after freeze or bundle build).
- `run`/`resume` print `PROMOTED: <ref> is at <sha>`, `DECIDED: winner <L>`,
  `REJECTED: <reason>` or `NEEDS_INTERVENTION: <reason>` as the last line.

## Tests added: 6 (and 4 crash points)

- `crates/horch-e2e/tests/promotion.rs`: `pro_08_default_collects_only`,
  `pro_08_promote_to_and_promote_cmd` (ff by `run --promote-to`, then
  cherry-pick by `promote`, then the refusal of a second promotion),
  `pro_02_e2e_only_valid_candidate_promotable` (a real gate
  `test ! -f bad.txt`; also `fail-gate:no-bad` from `run` gives exit 6),
  `pro_rollback_e2e`, `pro_cleanup_needs_force_from_intervention`.
- `crates/horch-e2e/tests/usage_dataset.rs`: `exp_07_candidate_and_judge_in_usage`.
- `cmp_13_crash_every_boundary`: `abort-during-cleanup:1`,
  `abort-after-promotion-started`, `abort-after-update-ref`,
  `abort-after-receipt`. A new helper `crash_and_resume_promoting` adds the
  checks: exactly 1 receipt, `release` reflog = creation + 1 move, COMPLETE.
- Changed (orchestrator-approved): `cmp_01_cli_args` now expects the hidden
  `promote`, `rollback`, `cleanup` to refuse an unknown round with exit 1.
- Changed (orchestrator-approved): fake-claude `judge_mode` writes the
  session transcript when `judge.json` has `usage`.

## Gotchas

- The sealed e2e PATH has no `sh`, and `CommandValidator` runs `sh -c`.
  A gate test must link `/bin/sh` into `h.bin` (promotion.rs does).
- `fail-gate:<name>` fails that gate for every candidate. To fail one
  candidate, use a real gate that reads the worktree.
- Two experiments minted within about 65 s share `exp8` (the first 8 hex
  digits of a UUIDv7), so their candidate branches collide and PRE-01
  refuses the second. The tests delete the last round's branches first.
- `promote` re-enters through `resume`, so it needs the experiment's
  `run.json`, manifest and preflight report.

## Outside my scope (not fixed)

- Codex candidate session discovery race: the worker looks for the codex
  session every 3 s; a candidate that ends in less than 3 s loses its
  session id (`no_session_id` in `horch cost`). The orchestrator handles it
  in a separate unit. fake-codex also writes no token counts.
- The `exp8` branch collision above: a real operator who starts 2
  experiments in the same minute in one repo gets a PRE-01 refusal.
- `run --promote-to <missing branch>` is not checked by preflight; the
  engine fails in REVALIDATING (exit 1) and the round stays there.
- `crates/horch-core/tests/execution_store.rs` still has the unused import
  warning (`KIND_ORCHESTRATOR`).

## SPEC-TODO

None new.
