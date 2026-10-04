# Report: U15 b4-evaluation

Branch `ard/b4-evaluation`. Phase B4 (pure part). Requirements JDG-03,
JDG-05, JDG-06, JDG-07, SEC-06.

## Files

| File | Content |
|---|---|
| `crates/horch-core/src/evaluation/mod.rs` | `pub mod judgment, parser, rubric, winner`, next to U14's `validator`; one merged doc comment |
| `crates/horch-core/src/evaluation/judgment.rs` | `Judgment`, `JudgmentVerdict`, `CandidateAssessment`, `JudgmentRecord`; `JUDGMENT_SCHEMA_VERSION`, `JUDGMENT_FIELDS`, `JUDGMENT_OPTIONAL_FIELDS`, `ASSESSMENT_FIELDS`, `VERDICTS` |
| `crates/horch-core/src/evaluation/parser.rs` | `ParseError`, `parse_judgment(raw, labels, cap_bytes)` |
| `crates/horch-core/src/evaluation/rubric.rs` | `RUBRIC_VERSION`, `SCORE_MIN`, `SCORE_MAX`, `rubric_text()`, `schema_text()`, `components()`, `judge_policy_digest(..)` |
| `crates/horch-core/src/evaluation/winner.rs` | `RejectReason`, `WinnerPolicy`, `TieBreak`, `WinnerOutcome`, `decide_winner` |
| `crates/horch-core/assets/judge/rubric-1.md` | provisional rubric, 5 components |
| `crates/horch-core/assets/judge/judgment-schema-1.0.0.json` | draft 2020-12 schema of `Judgment` |
| `crates/horch-core/tests/evaluation.rs` | 14 tests |
| `crates/horch-core/src/lib.rs` | `pub mod evaluation;` |

## Tests

14 integration tests in `tests/evaluation.rs`, 2 unit tests in `rubric.rs`.
`check-req-coverage.sh --phase B4` reports JDG-03, JDG-05, JDG-06, JDG-07 and
SEC-06 as ok. The other B4 IDs belong to later units.

## Decisions

- **Duplicate keys.** A private `Node` tree with a hand-written
  `Deserialize` visitor keeps every object member, duplicates too. A depth
  first walk reports the first repeated key as `DuplicateKey("$.a.b")`. Then
  the tree becomes a `serde_json::Value`. No new crate.
- **No serde error text is matched.** A shape walk over the `Value` compares
  keys with the field constants in `judgment.rs` and gives `UnknownField`,
  `MissingField` and `UnknownEnum` with a JSON path. Only after that does
  serde build the typed `Judgment`. Its remaining failure is a wrong JSON
  type, which maps to `NotJson("wrong type: ..")`. The serde message is put
  in the string for a reader only. ARC-22 is safe.
- **Wrong JSON type → `NotJson`.** The design's `ParseError` has no type
  variant. A top-level non-object and a field of the wrong type both give
  `NotJson`.
- **`NaN`, `Infinity`, `1e400` → `NotJson`.** They are not JSON, and
  serde_json rejects them at the tokenizer. `NonFinite` is for numbers that
  parse but are out of range: `confidence` outside 0..=1 or a score outside
  `SCORE_MIN..=SCORE_MAX` (0..=10).
- **`schema_version` other than `1.0.0` → `UnknownEnum`.**
- **Score keys.** The parser checks score keys against `components()`. An
  extra component is `UnknownField`, a missing one is `MissingField`.
- **`winner`.** It may be absent or null unless the verdict is `winner`.
  With verdict `winner` it is required and must be a label. A non-null
  `winner` on any other verdict is `ImpossibleLabel`.
- **Check order.** Cap, UTF-8, single JSON value, duplicates, shape, types,
  numbers, labels.
- **Policy digest.** Each of the 7 inputs enters as an 8-byte big-endian
  length and then its bytes. `WinnerPolicy` enters as `canonical_json`.
- **`decide_winner` order.** Empty `eligible` comes first, so `None` with
  no eligible label is `Rejected{NoEligible}`. A winner that is not eligible
  is `Rejected{JudgeRejected}` before the confidence check. Confidence equal
  to `min_confidence` is confident. A NaN on either side is not confident.
  A `winner` verdict with no label (only possible for a hand-built
  `Judgment`) gives `NeedsIntervention`.
- **Utility tie-break.** The tied labels are the eligible labels that the
  judge marked `acceptable`. The highest sum of component scores wins.
  Equal sums go to the first label in lexical label order. No acceptable
  eligible label gives `NeedsIntervention`. The sum is never stored.
  `min_confidence` does not apply to a tie.
- **`RejectReason`** has `Display` with the same snake_case spelling as
  serde. `BelowConfidence` and `Tie` are defined but `decide_winner` does not
  return them: the design table maps those cases to `NeedsIntervention`.
- **`JudgmentRecord`** derives `Serialize` and `Deserialize`, without
  `deny_unknown_fields`, because the design does not put it there.

## SPEC-RESOLVED (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3-§11, ai_docs/reports/finish/spec-b-preflight.md)

- `judgment.rs` `Judgment`: SPEC-RESOLVED(Spec B §11) the Judgment schema verbatim. (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §10/§11/§4.9, ai_docs/reports/finish/spec-b-judge.md)
- `winner.rs` `TieBreak`: SPEC-RESOLVED(Spec B §11) the utility definition. (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §10/§11/§4.9, ai_docs/reports/finish/spec-b-judge.md)
- `winner.rs` `decide_winner`: SPEC-RESOLVED(Spec B §11) the Abstain and RejectAll mappings. (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §10/§11/§4.9, ai_docs/reports/finish/spec-b-judge.md)
- `winner.rs` `utility_winner`: SPEC-RESOLVED(Spec B §11 utility) the tied labels and the rule. (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §10/§11/§4.9, ai_docs/reports/finish/spec-b-judge.md)
- `rubric-1.md` and the schema `description`: SPEC-RESOLVED(Spec B §10/§11) provisional. (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §10/§11/§4.9, ai_docs/reports/finish/spec-b-judge.md)

## For later units

- `build_judge_input` can put `rubric_text()` and `schema_text()` in the
  bundle and `sha256_bytes(schema_text())` in `schema_digest`.
- The judge job's output cap is 1 MiB (design §4.7). Pass it as `cap_bytes`.
- U13 `b1-measure` can replace its `RejectReason` JSON placeholder with
  `horch_core::evaluation::winner::RejectReason`.
- Master plan risk 4: if the judge often adds code fences, every answer is
  `NotJson`. Use `--json-schema` or reword the rubric. Do not relax the
  parser.
