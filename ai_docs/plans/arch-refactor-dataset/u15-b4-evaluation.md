# U15 b4-evaluation: Judgment, strict parser, winner policy, rubric, policy digest

Unit slug: `b4-evaluation`. Branch: `ard/b4-evaluation`. Phase: B4 (pure part).
Requirements: JDG-03, JDG-05, JDG-06, JDG-07, SEC-06 (parser caps).

## GOAL

The pure judge logic exists and is tested: the `Judgment` type, a strict
parser that never repairs input, the deterministic winner policy, the
versioned rubric and judgment schema files, and the judge policy digest.
No process, no file I/O beyond reading the compiled-in rubric and schema.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §2 "evaluation", §3 "B4", §4 JDG and SEC rows,
  §6 risk 4 (strict parsing vs code fences).
- The design doc is your main spec:
  `ai_docs/designs/2026-10-02-dataset-competition-design.md` §4.7 (Judge:
  `Judgment`, `JudgmentVerdict`, `CandidateAssessment`, `ParseError`,
  `parse_judgment`, `WinnerPolicy`, `TieBreak`, `WinnerOutcome`,
  `decide_winner` and its table, `judge_policy_digest`). Implement exactly
  that. Do NOT implement `build_judge_input`, the scheduler, or
  `headless_command` (later B4 units).
- Spec B §10 and §11 text is not available. The design marks the Judgment
  shape, the utility tie-break, and the Abstain/RejectAll mappings
  `SPEC-TODO(Spec B §11)`. Keep those markers.
- Ownership: you own `RejectReason` (design §4.1 lists it among event
  payloads; U13 `b1-measure` uses a JSON placeholder until your type lands).
  Define it in `evaluation/winner.rs` with the variants
  `NoEligible, JudgeRejected, BelowConfidence, Tie, StaleJudgment, RevalidationFailed`
  (serde snake_case).
- Merged building blocks: `horch_core::measure::digest::{Digest, sha256_bytes, digest_json}`,
  `horch_core::ids::{RoundId, JudgmentId, ExecutionId}`.
- U14 `b2-vcs` creates `evaluation/mod.rs` and `evaluation/validator.rs` at the
  same time. Create `evaluation/mod.rs` yourself if it is not on your base;
  a trivial conflict on its `pub mod` lines is expected; keep both sides.

## FILES

own:
- `crates/horch-core/src/evaluation/{judgment,parser,winner,rubric}.rs` (new)
- `crates/horch-core/src/evaluation/mod.rs` (your lines)
- `crates/horch-core/src/lib.rs` (add `pub mod evaluation;` if absent)
- `crates/horch-core/assets/judge/rubric-1.md` (new)
- `crates/horch-core/assets/judge/judgment-schema-1.0.0.json` (new)
- `crates/horch-core/tests/evaluation.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/b4-evaluation.md`

do not touch: every other file.

## STEPS

1. Create the worktree (conventions §2).
2. `evaluation/judgment.rs`: `Judgment`, `JudgmentVerdict`,
   `CandidateAssessment`, `JudgmentRecord` per design, with
   `#[serde(deny_unknown_fields)]` where the design puts it.
3. `crates/horch-core/assets/judge/judgment-schema-1.0.0.json`: a JSON Schema
   (draft 2020-12) that describes exactly the `Judgment` shape
   (`additionalProperties: false` everywhere, enums, `confidence` 0..1).
   `crates/horch-core/assets/judge/rubric-1.md`: a provisional rubric with
   named components (for example `correctness`, `tests`, `scope`,
   `maintainability`, `risk`), each scored 0..10, with one paragraph per
   component, and the instruction to answer with the JSON only. Head it with
   `SPEC-TODO(Spec B §10/§11): provisional until the spec text arrives.`
   `evaluation/rubric.rs`: `RUBRIC_VERSION = "rubric-1"`,
   `rubric_text()` and `schema_text()` via `include_str!`, `components()`
   (the component names parsed from the rubric, used by the parser to check
   score keys), and `judge_policy_digest(...)` per design: sha256 over the
   7 inputs, each length-prefixed (8-byte big-endian length, then bytes) so
   that no concatenation is ambiguous; `WinnerPolicy` enters as its
   canonical JSON.
4. `evaluation/parser.rs`: `parse_judgment(raw, labels, cap_bytes)`:
   - `raw.len() > cap_bytes` → `TooLarge`.
   - Not UTF-8 or not a single JSON value → `NotJson`. Leading or trailing
     non-whitespace (for example a code fence) → `NotJson`. Never strip.
   - Duplicate keys at any level → `DuplicateKey(path)`. serde_json keeps the
     last duplicate silently for maps, so first scan with a small
     hand-written checker over `serde_json::Value` built from a custom
     `Deserialize` visitor that records duplicates (or a tokenizer pass).
     No new crate.
   - Unknown field → `UnknownField`; unknown enum value → `UnknownEnum`;
     missing field → `MissingField`. Map serde errors to these by matching
     on the error category and position, not on message text if avoidable;
     if you must match message text, isolate it in one function with a
     test per case (ARC-22 later forbids error-string matching in
     application code; record this in the report).
   - Any non-finite or out-of-range number (`confidence` outside 0..=1, a
     score outside the rubric range) → `NonFinite` or `ImpossibleLabel` as
     the design names them.
   - `ranking` must hold every label in `labels` exactly once;
     `candidates` keys must equal `labels`; `winner` must be a label and is
     required iff verdict = winner → `ImpossibleLabel`.
5. `evaluation/winner.rs`: `RejectReason`, `WinnerPolicy` (default
   `min_confidence: 0.7`, `tie_break: Disabled`), `TieBreak`,
   `WinnerOutcome`, `decide_winner` exactly per the design table. For
   `TieBreak::Utility`, implement the simplest deterministic rule (highest
   sum of component scores among the tied labels, then label order) and
   mark it `SPEC-TODO(Spec B §11 utility)`.
   Component scores are kept as they are; nothing collapses them into one
   number stored anywhere.
6. Tests in `crates/horch-core/tests/evaluation.rs`:
   - `jdg_03_policy_digest_changes_with_inputs`: changing any one of the 7
     inputs changes the digest; the same inputs give the same digest.
   - `jdg_05_unknown_enum`, `jdg_05_missing_field`, `jdg_05_duplicate`,
     `jdg_05_impossible_label`, `jdg_05_non_finite`: one each.
   - `jdg_05_malformed_no_promotion`: a fenced JSON (```json ... ```) and a
     JSON with a trailing comma both fail; `decide_winner(None, ..)` then
     gives `NeedsIntervention` (no winner).
   - `jdg_06_policy_table`: every row of the design table.
   - `jdg_07_tie_needs_intervention_by_default`.
   - `sec_06_caps_enforced`: an input 1 byte over the cap → `TooLarge`.
   - `judgment_schema_matches_type`: the schema's property names equal the
     `Judgment` field names (parse the schema JSON and compare key sets).
7. Gate after each step. Commits: `B4: Add Judgment types and schema`,
   `B4: Add rubric and judge policy digest`, `B4: Add strict judgment parser`,
   `B4: Add winner policy`, `B4: Add evaluation tests`.
8. Write and commit the report. Follow conventions §6.

## DONE WHEN

- The named tests pass. `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: API, the duplicate-key method, any error-text
  matching, every SPEC-TODO.
