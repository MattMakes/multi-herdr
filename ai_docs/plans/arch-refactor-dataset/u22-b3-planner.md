# U22 b3-planner: round state machine, candidate planner, diversity, budget policy

Unit slug: `b3-planner`. Branch: `ard/b3-planner`. Phase: B3 (pure part).
Requirements: CMP-03, CMP-06, EXP-02.

## GOAL

The pure decision core of a competition round exists and is tested: the
round state machine with an explicit transition table (both the default and
the `--promote-to` path), the candidate planner (baseline slot through the
normal routing gate, greedy diversity, one seeded exploration slot with its
propensity), and the budget policy that the coordinator will apply live.
No process, no herdr, no git.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: OD4, OD5, §3 "B3" items 4 and 5 (planner,
  budget), §3 "B5" first paragraph (state machine with OD5), §4 CMP-03,
  CMP-06, EXP-02 rows.
- Design doc `ai_docs/designs/2026-10-02-dataset-competition-design.md` §5
  (round state machine: every state and transition, default and promote
  paths) and §4.1 (`RoundCreated`, `CandidatePlanned`, `SlotKind`).
- Merged code and reports (`ai_docs/reports/arch-refactor-dataset/`):
  - `b1-measure.md`: `competition::model::{RoundState, Round, Candidate, CandidateLabel}`,
    `measure::projection::fold` (it has its own transition handling today:
    make it use your table), event payloads `RoundCreated`,
    `CandidatePlanned`, `SlotKind`.
  - `a5-routing.md`: `routing::eligible::{eligible_fallbacks, roster_eligibility, EligibilityFilter, EligibleEntry, ExclusionReason}`,
    `routing::decision::{decide, resolve, GateFlags, Decision, RoutingMode}`.
    `HarnessKind` has no `Ord`.
  - `b1-primitives.md`: `measure::testkit::{SplitMix64, seed_from_digest, property}`,
    `measure::digest`.
  - `b2-preflight.md`: `competition::config::DatasetConfig` (baseline,
    exclude, budget, caps).
  - `machine-teacher.md`: `teacher::TeacherRef::none()`.
- Parallel units: U23 `b4-judge-input`, U24 `b6-export`, and the A4/A6/A7/A11
  refactors. You own only the files below.

## FILES

own:
- `crates/horch-core/src/competition/{state,planner,diversity,budget}.rs` (new)
- `crates/horch-core/src/competition/mod.rs` (your lines)
- `crates/horch-core/src/measure/projection.rs` (only to call the state table)
- `crates/horch-core/src/harness/mod.rs` (only to derive `PartialOrd, Ord` on
  `HarnessKind`, if you need it; U18 also edits this file, so add the derive
  in its own commit and touch no other line)
- `crates/horch-core/tests/competition_planner.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/b3-planner.md`

do not touch: every other file.

## STEPS

1. Create the worktree (conventions §2).
2. `competition/state.rs`: `pub fn transition(from: RoundState, event: &RoundEvent) -> Result<RoundState, InvalidTransition>`
   where `RoundEvent` is the subset of event kinds that move a round (map from
   `EventKind`). One `const TABLE: &[(RoundState, &str, RoundState)]` holds
   every allowed transition for both paths in design §5 (default:
   `… → JUDGING_BACKGROUND → DECIDED → CLEANUP → COMPLETE`; promote:
   `DECIDED → revalidate → PROMOTING → PROMOTED | NEEDS_INTERVENTION | REJECTED → CLEANUP → COMPLETE`;
   `promote <round>` re-enters at DECIDED; NEEDS_INTERVENTION keeps
   everything and reaches CLEANUP only by an operator cleanup command).
   `projection.rs` uses `transition` and records an `Anomaly` on error.
3. `competition/planner.rs` + `diversity.rs`:
   `pub fn plan_round(input: &PlanInput) -> RoundPlan` (pure) where
   `PlanInput { round_id, n, baseline: Option<TeammateName>, roster, view, filter, config }`
   and `RoundPlan { candidates: Vec<PlannedSlot>, eligible_set: Vec<EligibleEntry>, propensities: BTreeMap<String, f64>, seed: u64, teacher: TeacherRef, label_policy_version: String }`.
   - Slot 1 baseline: the `--baseline` teammate (or config) routed through
     `routing::decision::decide` + `resolve` exactly as `horch spawn` does
     (mode Auto, no flags), so a substitution or refusal shows up here.
     If refused, there is no baseline slot; record why.
   - Slots 2..n−1 diversity: from `roster_eligibility` (eligible entries
     only), greedily pick the entry that adds the most new distinct values of
     (harness, model, effort), ties broken by a stable order (teammate name).
   - Last slot exploration (when n ≥ 2): draw uniformly among the remaining
     eligible entries with `SplitMix64` seeded by
     `seed_from_digest(sha256(round_id))`; propensity = 1/k where k is the
     number of remaining eligible entries. Baseline and diversity slots have
     propensity 1.0.
   - Labels: `CandidateLabel::from_index` in slot order (the judge's
     anonymous shuffle happens later in the bundle).
   - `teacher` is `TeacherRef::none()`. `config_id` per candidate =
     `<teammate>|<harness>|<model>|<effort or ->`.
   - `round_created_payload(&RoundPlan)` and `candidate_planned_payload(slot)`
     build the B1 event payloads.
4. `competition/budget.rs`: pure `BudgetPolicy::check(spent: MicroUsd, committed: MicroUsd, config) -> BudgetAction { Continue, StopLaunches, CancelRunning }`
   with: StopLaunches when spent + committed ≥ hard − judge_reserve;
   CancelRunning when spent ≥ hard − judge_reserve; document the rule.
5. Tests in `crates/horch-core/tests/competition_planner.rs`:
   - `cmp_03_transition_table`: every TABLE row is accepted; a sample of
     invalid pairs is rejected; both paths reach COMPLETE.
   - `cmp_03_prop_no_invalid_path_to_promoted`: `property` with 10 000
     random event sequences (fixed seed); no sequence reaches PROMOTED
     without passing DECIDED, revalidation and PROMOTING in order.
   - `cmp_06_planner_deterministic`: same input → identical plan; a
     different round_id may change only the exploration slot.
   - `cmp_06_propensities`: baseline and diversity 1.0, exploration 1/k;
     over 10 000 round ids the exploration pick frequency of each eligible
     entry is within 3 % of 1/k.
   - `cmp_06_eligible_set_persisted`: the `round.created` payload holds the
     full eligible set with exclusion reasons and the propensities.
   - `cmp_06_baseline_routed_through_gate`: with a quota fixture that forces
     a substitution (reuse the routing test fixtures), the baseline slot is
     the substituted teammate and the plan records the substitution.
   - `exp_02_round_records_teacher_none`.
   - `budget_policy_table`.
6. Gate after each step. Commits: `B3: Add round state table`,
   `B3: Add candidate planner and diversity`, `B3: Add budget policy`,
   `B3: Add planner tests`.
7. Write and commit the report. Follow conventions §6.

## DONE WHEN

- The named tests pass. `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: API for the coordinator, the transition table, gotchas.
