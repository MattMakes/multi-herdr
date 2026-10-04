# T4 spec-b-judge: Spec B judge — close its SPEC-TODOs

Unit slug: `spec-b-judge`. Branch: `ds/spec-b-judge`.

## GOAL

Close every SPEC-TODO in: Spec B §10 (the judge evaluator prose, in teammates/judge.md) and §11 (Judgment schema, tie-break utility, Abstain and RejectAll mapping), and the System One criteria semantics. The design text states the implemented,
tested behaviour as the spec, and no marker in your files remains.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md` ("Closing a SPEC-TODO").
- Your markers: dataset design SPEC-TODO(Spec B §10), (Spec B §11) x3, (System One criteria semantics); crates/horch-core/src/evaluation/{judgment,winner}.rs, teacher/system_one.rs, assets/judge/{judgment-schema-1.0.0.json,rubric-1.md}, tests/judge_input.rs markers; teammates/judge.md. The judgment schema is versioned: a changed schema is a new version file, never an edit of 1.0.0.
- List them first with `grep -rn SPEC-TODO` on your files. A marker in a plan
  or report under `ai_docs/plans` or `ai_docs/reports` is NOT yours: unit T6
  updates those after you merge, using your report.
- Other units edit other sections of the same design file. Keep your edits
  inside your sections so git merges them cleanly.

## FILES

own: those sections of the dataset design; evaluation/{judgment,winner}.rs; teacher/system_one.rs; assets/judge/*; teammates/judge.md; tests/judge_input.rs; `ai_docs/reports/finish/spec-b-judge.md`.

## STEPS

0. Create the worktree.
1. Close the markers, one commit per section, gate before each commit.
2. Report (the per-marker table). Follow the merge protocol.
