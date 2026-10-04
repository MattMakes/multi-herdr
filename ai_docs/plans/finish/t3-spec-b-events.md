# T3 spec-b-events: Spec B events and states — close its SPEC-TODOs

Unit slug: `spec-b-events`. Branch: `ds/spec-b-events`.

## GOAL

Close every SPEC-TODO in: Spec B §20 (non-goals), the event list, the WorkerRun field list, the round states. The design text states the implemented,
tested behaviour as the spec, and no marker in your files remains.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md` ("Closing a SPEC-TODO").
- Your markers: dataset design (ai_docs/designs/2026-10-02-dataset-competition-design.md) SPEC-TODO(Spec B §20), (Spec B event list), (Spec B WorkerRun), (Spec B round states); crates/horch-core/src/measure/{event,projection,worker_run}.rs and competition/{state,model}.rs markers.
- List them first with `grep -rn SPEC-TODO` on your files. A marker in a plan
  or report under `ai_docs/plans` or `ai_docs/reports` is NOT yours: unit T6
  updates those after you merge, using your report.
- Other units edit other sections of the same design file. Keep your edits
  inside your sections so git merges them cleanly.

## FILES

own: those sections of the dataset design; measure/{event,projection,worker_run}.rs; competition/{state,model}.rs; `ai_docs/reports/finish/spec-b-events.md`.

## STEPS

0. Create the worktree.
1. Close the markers, one commit per section, gate before each commit.
2. Report (the per-marker table). Follow the merge protocol.
