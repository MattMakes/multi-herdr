# Report: T6 spec-history (resolve history markers, guard against new ones)

Unit `spec-history`, worker sonnet-13, 2026-10-04, committed on `design-skills`.
Plan: `ai_docs/plans/finish/t6-spec-history.md`.

## Result

`git grep -n` for the unresolved-marker token prints only the two files the
plan excludes: `ai_docs/plans/finish/t6-spec-history.md` and
`ai_docs/plans/finish/00-conventions.md`.

## What changed

- 36 history files under `ai_docs/plans/` and `ai_docs/reports/` now say
  `SPEC-RESOLVED` where they said the open token. 120 lines changed.
- Lines in `ai_docs/plans/arch-refactor-dataset/`, `ai_docs/plans/design-skills/`,
  `ai_docs/reports/arch-refactor-dataset/` and `ai_docs/reports/design-skills/`
  end with `(closed: <design file> <section>, ai_docs/reports/finish/<unit>.md)`.
  The unit comes from keywords on the line (Spec A core, Spec A marketplace,
  Spec B events, judge, preflight). The section is a range of the design, not a
  single number, when the line names none.
- Lines in `ai_docs/plans/finish/t1..t5` and `ai_docs/reports/finish/*` got the
  token replacement only. They are the closing records themselves, and most
  are table rows where a suffix would break the table.
- No marker remained in a design file or a code file. T4 named
  `crates/horch-core/src/dataset/export.rs:45` and `execution/store.rs`; both are already closed.
- `ai_docs/gates/architecture-refactor/CHECKLIST.md`: status is now final.
  Item 12 is now "No re-export shim module exists (`arc_25_no_shim_modules` passes)".
- `ai_docs/gates/architecture-refactor/SPEC-COVERAGE.md`: it lists no open markers.
  I added a note that the markers are closed and that the gate guards them.
- `scripts/phase-gate.sh`: new first step `no_spec_todo`. It fails with the
  file:line list. The token is built from two parts so the script does not match itself.

## Evidence

- A temporary marker in `ai_docs/reports/finish/_tmp_marker.md` made the step exit 1
  and print its file:line. After removal the step exits 0.
- I ran the step alone, not the full gate (the plan asks for the step alone).
- `bash -n scripts/phase-gate.sh` passes.

## Not done, noticed outside scope

- `SPEC-COVERAGE.md` still has about 30 rows with status `PENDING spec text`
  and a "provisional" note. Its own final-audit rule wants zero such rows.
  The conventions say the implemented behaviour is the spec, so the rows can
  become `mapped`. I did not change them: they are not markers.
