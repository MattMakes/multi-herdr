# T7 spec-coverage: no row of SPEC-COVERAGE.md waits for spec text

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

`ai_docs/gates/architecture-refactor/SPEC-COVERAGE.md` has 0 rows with status
"PENDING spec text" (its final-audit rule requires 0). Each such row maps to
the design section and the test that now pin it.

## CONTEXT

- T1–T6 closed every SPEC-TODO: the implemented, tested behaviour is the
  spec (`ai_docs/plans/finish/00-conventions.md`). Their reports, with
  per-marker tables, are in `ai_docs/reports/finish/spec-*.md`.
- For each PENDING row: find the closed design section (grep the design
  files for the row's topic) and the test that pins it (the reports name
  them; check the test exists with `git grep -n 'fn <name>'`). Set the row's
  status to mapped, with `design §x.y` and the test name. If no test pins
  it, write the test (smallest assertion of the documented behaviour) or,
  if that is not possible, say why in the row and the report.
- The req-coverage script (`scripts/check-req-coverage.sh`) may read this
  file or the design tables: run it after your edit.

## FILES

own: `ai_docs/gates/architecture-refactor/SPEC-COVERAGE.md`, any new small
test file you need (name it in the report), `ai_docs/reports/finish/spec-coverage.md`.

## STEPS

1. List the PENDING rows. 2. Map each. 3. `scripts/check-req-coverage.sh`
and the tests you added. 4. COMMITTED note.
