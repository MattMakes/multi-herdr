# Report: T5 spec-b-preflight

Unit: `spec-b-preflight`. Branch: `ds/spec-b-preflight`. Worker: opus-55.

## Outcome

I closed 12 markers: 1 in the dataset design and 11 in code. The design
now states the implemented, tested behaviour as the spec. No `SPEC-RESOLVED`
remains in my files. I made no behaviour change. I added 6 tests that pin
behaviour that no test pinned before.

New design sections in `ai_docs/designs/2026-10-02-dataset-competition-design.md`:

- 4.11.1 Preflight checks and thresholds (Spec B §3).
- 4.11.2 Judge mode (Spec B §3).
- 4.10.1 Export row details (label policy, task features, score answers,
  Laya readiness, outcome scores).

I also aligned the `PreflightPlan` snippet in §4.11 with the code.

## Per-marker table

| # | Location | What it says now | Evidence | Code changed |
|---|---|---|---|---|
| 1 | design §4.11, `SPEC-RESOLVED(Spec B §3)` | §4.11.1: 13 thresholds, the safe-N formula, the Fail/Warn rule of PRE-01..PRE-13, the report, the gathered facts, 4 decisions | `competition/preflight.rs:evaluate`; tests in `crates/horch-core/tests/preflight.rs` and `crates/horch-e2e/tests/dataset.rs` | No |
| 2 | `competition/preflight.rs:9` module doc | Points to design 4.11.1 | as row 1 | Comment only |
| 3 | `competition/preflight.rs` `pre_07_providers` | A pool that is not `ok` warns, also `exhausted`; it never fails (decision 4) | new test `pre_07_a_pool_that_is_not_ok_warns_and_does_not_refuse` | Comment + test |
| 4 | `crates/horch/src/dataset/preflight.rs` build estimate | A never-built project counts 0 build bytes; the 10 GiB headroom covers it (decision 1) | new test `tree_bytes_counts_a_never_built_project_as_zero_build` | Comment + test |
| 5 | same file, local model size | `local_model_bytes` = 0; local candidates then run 1 at a time (decision 2) | `local_inference_bound`; `pre_04_local_inference_bound` | Comment only |
| 6 | same file, trusted directories | No trust store is read; PRE-13 warns for every root (decision 3) | `pre_13_herdr_and_horch_exe` | Comment only |
| 7 | `competition/config.rs` `JudgeMode::from_str` | `auto` is the only mode (design 4.11.2) | `cmp_01_cli_args`; new test `judge_mode_auto_is_the_only_mode` | Comment + test |
| 8 | `competition/planner.rs:LABEL_POLICY_VERSION` | `"slot-order-1"`, one path component (design 4.10.1) | new test `label_policy_version_is_slot_order_1_and_path_safe` | Comment + test |
| 9 | `dataset/export.rs:SCORE_KEY` | A score answer is `probabilities: {"score": <summed judge score>}` | `exp_03_row_shape`, `exp_04_export_golden` | Comment only |
| 10 | `dataset/export.rs:ExportState.task_features` | Exactly 4 keys: `budget_usd_micro`, `candidates`, `round_index`, `strategy` | `exp_03_row_shape`, `exp_04_export_golden` | Comment only |
| 11 | `dataset/readiness.rs:readiness` | Laya: ≥ 4 arms (`clef_min_arms`) with ≥ 50 runs each, plus ≥ 500 judged rounds | `exp_05_coverage_and_verdict` | Comment only |
| 12 | `crates/horch/src/dataset/cli.rs:OutcomeArg::default_score` | `verified` 1.0; `regression`, `revert` 0.0 | new test `outcome_default_score_per_kind` | Comment + test |

## Decisions (each with its reason, also in the design)

1. Build estimate: 0 for a project with no `target/`. A size with no
   measurement behind it would refuse rounds on a guess.
2. Local model size: 0. horch has no probe for it; 1 local candidate at a
   time keeps 1 model in memory.
3. Trusted directories: empty. The trust stores are private harness files,
   and LA-7 decides whether a trust prompt stalls a pane.
4. PRE-07 never fails on a pool state. The planner already drops candidates
   whose pool blocks a spawn, with the model scope. The pool-wide state can
   be stricter than the candidate's scope.
5. Judge mode: `auto` only. The master plan names only `--judge auto`.

## Gotchas

- PRE-12's report row repeats the PRE-06 harness-resolution test. The PRE-12
  requirement is the refusal itself (exit 4, no worktree, no model). I
  documented this and did not change the report shape.
- The gate takes more than 10 minutes. Run it with a long timeout.

## Found outside my scope (not fixed)

- `crates/horch/src/dataset/run.rs`: a plan with 0 candidates fails PRE-08,
  so `run` exits 4 (preflight failed) before it reaches the
  `exit::BUDGET_REFUSED` branch (exit 3, "no candidate can run within the
  usage limits"). That branch is unreachable. The master plan says exit 3 is
  the budget or quota refusal. A fix: check `plan.candidates.is_empty()`
  before preflight, or let PRE-08 not fail on 0 candidates.
- PRE-13 always warns in a real run, because no trust store is read.
  A follow-up after LA-7 can read the Claude and Codex trust stores.
- T6: the plan and report markers that point to these sections can now cite
  design 4.10.1, 4.11.1 and 4.11.2.
