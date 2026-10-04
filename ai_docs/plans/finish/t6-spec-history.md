# T6 spec-history: resolve the remaining markers and guard against new ones

Unit slug: `spec-history`. Branch: `ds/spec-history`.
Starts after T1–T5 merge (the orchestrator spawns you then).

## GOAL

`grep -rn 'SPEC-TODO' .` (outside `target/`) prints nothing, and the gate fails
if a new one appears.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md` and the T1–T5 reports
  in `ai_docs/reports/finish/`.
- The remaining markers are in plans and reports under `ai_docs/plans/` and
  `ai_docs/reports/` (history). Do not rewrite history: replace the token
  `SPEC-TODO` with `SPEC-RESOLVED` and append ` (closed: <design file> §<n>,
  ai_docs/reports/finish/<unit>.md)` on that line, so a reader can follow it.
- Any marker still in a design or code file means a T unit missed it: close it
  yourself following the conventions, and say so in the report.
- Add a gate step to `scripts/phase-gate.sh`: fail with the file:line list if
  `git grep -n 'SPEC-TODO'` finds anything, except this plan file and the
  conventions file, which describe the token (exclude them by path).
- Update `ai_docs/gates/architecture-refactor/SPEC-COVERAGE.md` if it lists
  open markers.

## FILES

own: every file under `ai_docs/plans/` and `ai_docs/reports/` that holds a
marker (only the marker lines), `scripts/phase-gate.sh`, SPEC-COVERAGE.md,
`ai_docs/reports/finish/spec-history.md`.

## STEPS

0. Create the worktree. 1. Markers. 2. Gate step (prove it fails on a
temporary marker, then remove it). 3. Gate. Report. Merge protocol.
