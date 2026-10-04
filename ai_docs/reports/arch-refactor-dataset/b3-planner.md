# U22 b3-planner: report

Branch `ard/b3-planner`. Phase B3 (pure part). Requirements CMP-03, CMP-06,
EXP-02. Gate green after every commit.

## What landed

| File | Content |
|---|---|
| `competition/state.rs` | `RoundEvent`, `TABLE`, `transition`, `InvalidTransition` |
| `competition/planner.rs` | `PlanInput`, `RoundPlan`, `PlannedSlot`, `Baseline`, `plan_round`, `round_created_payload`, `candidate_planned_payload`, `config_id`, `round_seed`, `LABEL_POLICY_VERSION` |
| `competition/diversity.rs` | `Coverage`, `pick_diverse` |
| `competition/budget.rs` | `BudgetPolicy::check`, `BudgetAction` |
| `measure/projection.rs` | every state change goes through `transition` |
| `tests/competition_planner.rs` | 9 tests (below) |

`harness/mod.rs` is not changed. The planner compares harnesses by
`as_str()`, so `HarnessKind` needs no `Ord`.

## API for the coordinator

```rust
let plan = plan_round(&PlanInput {
    round_id, index, base_sha, n, baseline /* --baseline */, roster, view,
    filter /* caller exclusions; config.exclude is added */, config,
});
// emit round.created, then candidate.planned per slot:
round_created_payload(&plan);
plan.candidates.iter().map(candidate_planned_payload);
// plan.baseline says why a baseline slot is missing (missing_reason()).

match BudgetPolicy::check(spent, committed, &config.budget) {
    BudgetAction::Continue | BudgetAction::StopLaunches | BudgetAction::CancelRunning => …
}
```

- Baseline: `--baseline`, else `config.baseline`. It goes through
  `routing::decision::decide` with `BalanceMode::Auto` and no flags, then
  `resolve`. A substitution plans the fallback teammate (name of the
  fallback, harness/model/effort of the merged teammate). A refusal, an
  unknown name or a reserved tier gives no baseline slot; diversity fills
  the slot instead, so a round still has `n` candidates when the roster
  allows.
- Diversity: eligible entries with a model, not yet planned (the
  baseline's requested and resolved names count as planned). Each pick
  maximizes new values of (harness, model, effort); a tie goes to the first
  teammate name.
- Exploration (n ≥ 2): uniform over the k remaining eligible entries,
  `SplitMix64::new(seed_from_digest(sha256(round_id)))`, 1 draw of
  `below(k)`. Propensity 1/k. Other slots 1.0.
- `propensities` is keyed by label. `teacher` is `TeacherRef::none()`.
- Budget: `limit = hard − judge_reserve`. `spent ≥ limit` → CancelRunning.
  `spent + committed ≥ limit` → StopLaunches. Hard 0 → CancelRunning
  (preflight PRE-09 refuses such a run first).

## The transition table

A `RoundEvent` is an event kind plus the condition that picks the target
(`candidate.planned:last`, `validation.completed:none_eligible`,
`judge.failed:last_attempt`, `round.needs_intervention:<source>`, …).
`candidate.completed` and `candidate.failed` both map to `candidate.ended`.

- Default: CREATED → PREFLIGHT → PLANNED → PROVISIONING → RUNNING →
  VALIDATING → JUDGING_BACKGROUND → DECIDED → CLEANUP → COMPLETE.
- Promote: DECIDED →(promotion.requested) REVALIDATING →(promotion.started)
  PROMOTING →(promotion.completed) PROMOTED → CLEANUP → COMPLETE.
  REVALIDATING → REJECTED | NEEDS_INTERVENTION; PROMOTING →
  NEEDS_INTERVENTION.
- NEEDS_INTERVENTION → DECIDED (`operator.promote`) or CLEANUP
  (`round.cleanup_started`, operator only).
- COMPLETE → DECIDED (`operator.promote`): see SPEC-TODO below.
- `promotion.rolled_back`: PROMOTED and COMPLETE, self-loop.

## Gotchas

- `winner.selected{promotion: requested}` now applies 2 transitions in the
  projection: `winner.selected` (→ DECIDED), then `promotion.requested`
  (→ REVALIDATING). The final state is the same as before.
- The projection computes the target state before it changes anything, so
  an invalid event changes nothing. `last` conditions are computed as
  "every other candidate already has X".
- `worktree.cleanup_failed` and `outcome.recorded` do not move a round and
  are not in the table. Their projection checks are unchanged.
- The fold cannot see who sent `round.cleanup_started`. The table allows it
  from NEEDS_INTERVENTION; the coordinator must emit it there only on the
  operator's `cleanup` command. Existing MEA-05 tests emit it with
  `Actor::Coordinator`, so I did not gate on the actor.
- `operator.promote` has no event kind yet. B5 must add one (or map
  `promote <round>` onto an existing kind) for the projection to use it.
- No "round deadline" event exists. The coordinator reaches VALIDATING by
  emitting a terminal event (`candidate.failed` TimedOut) per candidate.

## SPEC-TODO

- `SPEC-TODO(Spec B round states)`: states before JUDGING_BACKGROUND follow
  the B2/B3 flow (in `state.rs`).
- `SPEC-TODO(Spec B §promote)`: the row COMPLETE → DECIDED on
  `operator.promote`. The master plan says `promote <round>` "later
  re-enters at DECIDED"; design §5 lists only NEEDS_INTERVENTION.
- `SPEC-TODO(Spec B label policy)`: `LABEL_POLICY_VERSION = "slot-order-1"`.
  It names the export directory, so it must stay path-safe.

## Tests

`cmp_03_transition_table`, `cmp_03_prop_no_invalid_path_to_promoted`
(10 000 walks of 120 steps, fixed seed; asserts > 100 reach PROMOTED),
`cmp_06_planner_deterministic`, `cmp_06_propensities` (10 000 round ids,
each frequency within 0.03 absolute of 1/k), `cmp_06_eligible_set_persisted`,
`cmp_06_baseline_routed_through_gate` (fixture
`claude-exhausted-codex-ok`: opus → codex-sol; `all-exhausted`: refused),
`exp_02_round_records_teacher_none`, `budget_policy_table`,
`cmp_02_planner_files_have_no_adapter_imports`.
