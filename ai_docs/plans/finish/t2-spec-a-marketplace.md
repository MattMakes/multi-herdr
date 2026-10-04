# T2 spec-a-marketplace: Spec A marketplace and compatibility — close its SPEC-TODOs

Unit slug: `spec-a-marketplace`. Branch: `ds/spec-a-marketplace`.

## GOAL

Close every SPEC-TODO in: Spec A §10 (marketplace manifest key list), §13 (compatibility list), and the Spec B question at architecture design line ~615: whether the judge attempt needs its own idempotency key. The design text states the implemented,
tested behaviour as the spec, and no marker in your files remains.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md` ("Closing a SPEC-TODO").
- Your markers: architecture design SPEC-TODO(Spec A §10), (Spec A §13), (Spec B) near line 615; crates/horch-marketplace/src/manifest.rs.
- List them first with `grep -rn SPEC-TODO` on your files. A marker in a plan
  or report under `ai_docs/plans` or `ai_docs/reports` is NOT yours: unit T6
  updates those after you merge, using your report.
- Other units edit other sections of the same design file. Keep your edits
  inside your sections so git merges them cleanly.

## FILES

own: those sections of the architecture design; crates/horch-marketplace/src/manifest.rs; the judge idempotency code only if your decision changes it; `ai_docs/reports/finish/spec-a-marketplace.md`.

## STEPS

0. Create the worktree.
1. Close the markers, one commit per section, gate before each commit.
2. Report (the per-marker table). Follow the merge protocol.
