# T1 spec-a-core: Spec A core — close its SPEC-TODOs

Unit slug: `spec-a-core`. Branch: `ds/spec-a-core`.

## GOAL

Close every SPEC-TODO in: Spec A §3, §4 (RuntimeContext field grouping, Execution field list), §8 (worker field list, worker startup order), §16 (checklist items), §17 (criterion texts). The design text states the implemented,
tested behaviour as the spec, and no marker in your files remains.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md` ("Closing a SPEC-TODO").
- Your markers: the architecture design (ai_docs/designs/2026-10-02-architecture-refactor-design.md) markers SPEC-TODO(Spec A §3), (§4) x2, (§8) x2, (§16), (§17), plus line 15, which describes the marker convention (rewrite it to say the markers are closed); crates/horch-core/src/execution/{lifecycle,model,store}.rs markers.
- List them first with `grep -rn SPEC-TODO` on your files. A marker in a plan
  or report under `ai_docs/plans` or `ai_docs/reports` is NOT yours: unit T6
  updates those after you merge, using your report.
- Other units edit other sections of the same design file. Keep your edits
  inside your sections so git merges them cleanly.

## FILES

own: those sections of the architecture design; the execution/*.rs marker lines and any code fix there; `ai_docs/reports/finish/spec-a-core.md`.

## STEPS

0. Create the worktree.
1. Close the markers, one commit per section, gate before each commit.
2. Report (the per-marker table). Follow the merge protocol.
