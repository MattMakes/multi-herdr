# Report: spec-b-judge (T4)

Unit: `spec-b-judge`. Branch: `ds/spec-b-judge`. Worker: opus-54.
Plan: `ai_docs/plans/finish/t4-spec-b-judge.md`.

## Result

I closed all 14 markers this unit owns. The dataset design now states the
implemented, tested behaviour as the spec for Spec B §10 (evaluator prose),
Spec B §11 (Judgment schema, utility tie-break, Abstain and RejectAll) and
the System One `criteria` semantics. `grep -rn SPEC-TODO` finds no marker
in the files I own. The full gate was green on the §11 and §10 commits.
After rule change 2, the later commits had targeted checks only (see Checks).

## Commits

1. `Judge: Close Spec B §11 SPEC-TODOs: ...` (schema 1.0.1, rule u1, winner table).
2. `Judge: Close Spec B §10 SPEC-TODOs: ...` (judge.md, rubric-2).
3. `Teacher: Close the System One criteria SPEC-TODOs`.
4. This report.

## Markers closed

| # | Location | What it says now | Evidence | Code changed |
|---|---|---|---|---|
| 1 | design §4.7 `Judgment` comment (was line 626) | "Spec B §11: the judge's answer. JSON Schema: judgment-schema-1.0.1.json", `schema_version` "1.0.1" | `evaluation/judgment.rs:Judgment`, `judgment_schema_matches_type` | yes: version 1.0.1 |
| 2 | design §4.7 `TieBreak` comment (was line 665) | `v: "u1"` (`UTILITY_RULE_V1`) is the only rule | `evaluation/winner.rs:UTILITY_RULE_V1`, `an_unknown_utility_rule_needs_the_operator` | yes |
| 3 | design §4.7 after the winner table (was line 690) | Full table with 2 new rows, then the Judgment 1.0.1 spec (field list, parser checks, ParseError list), the Abstain/RejectAll reasons, and the utility rule u1 | `decide_winner`, `parse_judgment`, `jdg_05_*`, `jdg_06_policy_table`, `jdg_07_*`, 2 new unit tests in `winner.rs` | yes |
| 4 | design §4.9 `criteria` comment (was line 755) | opaque pass-through, see the new paragraph | `teacher/system_one.rs:Question`, `exp_01_system_one_serde_shape` | no |
| 5 | design §6 B4 (was line 1016) | The §10 evaluator prose spec: 5 points, the rubric-2 description, tests | `teammates/judge.md`, `harness/headless.rs:judge_prompt`, `jdg_01_judge_teammate_check` | no (prose kept) |
| 6 | `evaluation/judgment.rs:Judgment` doc | pointer to design §4.7 | - | comment only |
| 7 | `evaluation/winner.rs:TieBreak` doc | rule u1 doc, pointer to §4.7 | - | yes (const added) |
| 8 | `evaluation/winner.rs:decide_winner` doc | Abstain/RejectAll reasons, pointer to §4.7 | - | yes (2 new arms) |
| 9 | `evaluation/winner.rs:utility_winner` doc | rule u1 text | - | no |
| 10 | `teacher/system_one.rs:Question.criteria` comment | pass-through semantics, pointer to §4.9 | - | comment only |
| 11 | `assets/judge/judgment-schema-1.0.0.json` description | replaced by `judgment-schema-1.0.1.json` (no marker) | `judgment_schema_matches_type` | yes: new version |
| 12 | `assets/judge/rubric-1.md` line 1 | replaced by `rubric-2.md` (no marker) | `the_rubric_names_five_components`, `jdg_03_*` | yes: new version |
| 13 | `teammates/judge.md` line 13 | marker line removed; prose kept | `jdg_01_judge_teammate_check` | no |
| 14 | `tests/judge_input.rs` assertion of the marker | asserts the opening sentence, no marker (the word is split so grep does not match), and every bundle file name | `jdg_01_judge_teammate_check` | test changed |

## Checks before READY-TO-MERGE (after rebase on design-skills)

- `cargo build --workspace --all-targets`
- `cargo test -p horch-core --lib` and `--test` evaluation, judge_input, judging, dataset_export
- `cargo test -p horch-e2e --test judge`
- `cargo clippy -p horch-core -p horch-e2e --all-targets -- -D warnings`
- `rustfmt --edition 2021 --check` on the changed .rs files
- `horch teammates --check` with `HORCH_TEAMMATES_DIR=teammates`

## Decisions

- **Schema version 1.0.1 (option 1).** The orchestrator asked me to check
  whether the schema bytes reach the judge prompt or a recorded digest.
  Both are true: `harness/headless.rs:judge_prompt` substitutes
  `schema_text()` for `{schema}`; `judge_input.rs` writes `schema.json`
  and `schema_digest` into the bundle manifest (the bundle digest covers
  the manifest); `competition/judging.rs` puts `schema_text()` into the
  judge policy digest. So a text change is a new version. 1.0.1 differs
  from 1.0.0 only in `description`, `$id` and the `schema_version` const.
  I deleted `judgment-schema-1.0.0.json`: no code path loads it.
- **API enforcement note.** On the orchestrator's NOTE (opus-61 finding),
  design §4.7 now says that `--json-schema` gets the schema without its
  top-level `$schema` and `allOf`/`anyOf`/`oneOf` keys, so the conditional
  `winner` rule and the label rules are enforced locally by
  `parse_judgment`. The stripping code is opus-61's change, not mine; if it
  is not merged yet, the sentence describes its planned behaviour.
- **rubric-2.** `RUBRIC_VERSION` is "rubric-2". I deleted `rubric-1.md`:
  `evaluation/rubric.rs` was its only loader, and nothing has shipped, so
  no real round recorded rubric-1.
- **A winner the judge marked not acceptable needs the operator.** Before,
  `decide_winner` returned Winner. The rubric defines `winner` as an
  acceptable candidate, so the answer contradicts itself. rubric-2 also
  tells the judge to mark the winner acceptable. Test:
  `a_winner_the_judge_marked_unacceptable_needs_the_operator` (fails
  before, passes after).
- **Only utility rule "u1" exists.** Before, any `v` ran the u1 rule. Now
  another `v` needs the operator, so a policy never claims a rule that the
  code does not have. Test: `an_unknown_utility_rule_needs_the_operator`.
  Production always uses `WinnerPolicy::default()` (tie-break disabled).
- **Abstain and RejectAll kept.** RejectAll → Rejected{JudgeRejected};
  Abstain → NeedsIntervention. The reasons are in design §4.7.
- **`confidence` does not gate a utility tie-break.** Kept as built; the
  operator opts in to the tie-break.
- **System One `criteria` stays opaque.** OD4 and master plan item 7 say
  the wire details are unverified until Clef is enabled. horch writes
  `None` in every exported question and never reads the value.

## Files touched

- `ai_docs/designs/2026-10-02-dataset-competition-design.md` (§4.7, §4.9, §6 B4 only)
- `crates/horch-core/assets/judge/judgment-schema-1.0.0.json` → `judgment-schema-1.0.1.json`
- `crates/horch-core/assets/judge/rubric-1.md` → `rubric-2.md`
- `crates/horch-core/src/evaluation/{judgment,rubric,winner}.rs`
- `crates/horch-core/src/teacher/system_one.rs` (comment only)
- `teammates/judge.md` (marker line removed)
- Tests: `crates/horch-core/tests/{judge_input,evaluation,judging,dataset_export}.rs`,
  `crates/horch-e2e/src/bin/fake-claude.rs` (judgment fixtures "1.0.0" → "1.0.1",
  rubric version asserts). The orchestrator gave me the fixture edits.
- `evaluation/rubric.rs` is not in my `own` list; it holds the two
  version constants and `include_str!` paths, so the version bump needs it.

No oracle or golden changed. The export golden's "schema_version":"1.0.0"
entries are WorkerRun records, not judgments.

## Outside my scope (not fixed)

- `crates/horch-core/src/dataset/export.rs:45`: `SPEC-TODO(System One score answers)`.
- `crates/horch-core/src/execution/store.rs:94` and
  `ai_docs/designs/2026-10-02-architecture-refactor-design.md:615`:
  `SPEC-TODO(Spec B)` (judge attempt key).
- `crates/horch-core/src/competition/config.rs:104`: `SPEC-TODO(Spec B §3)`.
- Design Appendix B still says "PENDING: the orchestrator inserts the
  operator's Spec B text here."
- Markers in `ai_docs/plans` and `ai_docs/reports` are for unit T6.
