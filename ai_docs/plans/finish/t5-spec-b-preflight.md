# T5 spec-b-preflight: Spec B preflight, planner, export — close its SPEC-RESOLVEDs

Unit slug: `spec-b-preflight`. Branch: `ds/spec-b-preflight`.

## GOAL

Close every SPEC-RESOLVED in: Spec B §3 (preflight check list and thresholds), the planner, config, export, readiness and dataset CLI markers. The design text states the implemented,
tested behaviour as the spec, and no marker in your files remains.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md` ("Closing a SPEC-RESOLVED").
- Your markers: dataset design SPEC-RESOLVED(Spec B §3); crates/horch-core/src/competition/{preflight,planner,config}.rs, dataset/{export,readiness}.rs, crates/horch/src/dataset/{cli,preflight}.rs markers.
- List them first with `grep -rn SPEC-RESOLVED` on your files. A marker in a plan
  or report under `ai_docs/plans` or `ai_docs/reports` is NOT yours: unit T6
  updates those after you merge, using your report.
- Other units edit other sections of the same design file. Keep your edits
  inside your sections so git merges them cleanly.

## FILES

own: that section of the dataset design; those code files; `ai_docs/reports/finish/spec-b-preflight.md`.

## STEPS

0. Create the worktree.
1. Close the markers, one commit per section, gate before each commit.
2. Report (the per-marker table). Follow the merge protocol.
