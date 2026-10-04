# T2 spec-a-marketplace report

Unit: `spec-a-marketplace`. Branch: `ds/spec-a-marketplace`. Worker: opus-52.

## Result

I closed 4 SPEC-TODO markers: 3 in the architecture design
(`ai_docs/designs/2026-10-02-architecture-refactor-design.md`) and 1 in
`crates/horch-marketplace/src/manifest.rs`. The design now states the
implemented, tested behaviour as the spec. No code behaviour changed. The
only code change is the `SkillManifest` doc comment.

## Markers closed

| Location | What it now says | Evidence | Code changed |
|---|---|---|---|
| Design §4.10, the `// manifest.rs` comment (was `SPEC-TODO(Spec A §10)`) | A pointer to the new "SKILL.md manifest keys (Spec A §10)" table in §4.10 | — | no |
| Design §4.10, new table after "Rejection rules" | The frontmatter uses only `name`, `description`, `license`, `metadata`, `allowed-tools`, each with its required flag, type and rule. Any other key (`hooks`) is a `Manifest` error. Name and description rules are `SkillMd`. A symlinked or non-UTF-8 `SKILL.md` is `Manifest`. | `manifest.rs:SkillManifest::parse`, `SkillManifest::read`, `model.rs:is_valid_skill_name`; tests `manifest.rs:tests::accepts_known_keys`, `tests::rejects_bad_frontmatter`, `tests/marketplace.rs:mkt_06_rejects_hooks`, `mkt_06_rejects_invalid_skill_md` | no |
| `crates/horch-marketplace/src/manifest.rs` `SkillManifest` doc (was `SPEC-TODO(Spec A §10)`) | The 5 allowed keys, `hooks` rejected, and a pointer to design §4.10 (Spec A §10) | as above | comment only |
| Design §6 Compatibility (was `SPEC-TODO(Spec A §13)`) | The §6 table is the Spec A §13 compatibility list. It is complete: 6 artifacts (ledgers, briefs, frontmatter, offline skills, decision JSON, CLI). A rule changes only together with its test. Each cited test is mapped to its file. | Every cited test exists: `arc_17_*` (`execution_store.rs`, `execution_plan.rs`), `arc_07_brief_v1_readable`, `arc_08_legacy_frontmatter_corpus_parses`, `arc_03_harness_kind_serde_compat`, `skl_01_*`, `mkt_08_offline_reinstall_from_lock`, `mkt_08_runtime_needs_no_network`, `mkt_09_legacy_skills_flags_output_unchanged`, `arc_12_decisions_match_baseline`, `bal_04_*` | no |
| Design §4.3 (was `SPEC-TODO(Spec B)`: whether the judge attempt needs its own key) | Decision: no own key. `round_id` plus `label: "judge:<attempt>"` is the key. `find_judge` finds the attempt by that pair. Judge events carry `judge.<step>:<round>:<attempt>` keys. `Execution::idempotency_key` stays Candidate-only. | `execution/store.rs:kind_of`, `from_execution`, `JUDGE_LABEL`; `competition/judging.rs:find_judge`, `judge_execution`; `execution/model.rs:Execution::idempotency_key`; tests `judging.rs:jdg_09_judge_execution_in_ledger`, `jdg_04_resume_after_completed_writes_once`, `execution_plan.rs:arc_17_execution_conversion_lossless` | no |

## Decisions

- Judge key: the pair `(round, attempt)` already makes the judge record
  idempotent, so a separate key adds a second source of truth. A judge key in
  `Execution::idempotency_key` would also put judges into the coordinator's
  candidate map (`competition/coordinator.rs`, the map of candidate
  executions by key).
- §13: the original Spec A text is not available. Per the operator decision
  of 2026-10-04, the implemented table with its tests is the spec.

## Gotchas

- `crates/horch-core/src/execution/store.rs:94` carries the same
  `SPEC-TODO(Spec B)` judge question. T1 (opus-51) owns that file. The
  orchestrator confirmed that T1 replaces the marker with a pointer to design
  §4.3 ("The judge attempt key (Spec B)").
- The judge label parse error (`judge:` without a number gives
  `LegacyError::Kind`) has no dedicated test. The design states it from the
  code (`store.rs:kind_of`).

## Not done / follow-ups

- T6 updates the plan and report files under `ai_docs/plans` and
  `ai_docs/reports` that still mention these markers (for example
  `ai_docs/reports/arch-refactor-dataset/a8-marketplace.md`, `a6b-service.md`,
  `ai_docs/plans/arch-refactor-dataset/STATUS.md`).
