# T7 spec-coverage report

Result: `ai_docs/gates/architecture-refactor/SPEC-COVERAGE.md` has 0 rows with
status `PENDING spec text`. The file had 22 such rows (9 of Spec A, 13 of Spec B).

Each row now reads `mapped: design <section>; tests <names>`. `A` is
`ai_docs/designs/2026-10-02-architecture-refactor-design.md`. `B` is
`ai_docs/designs/2026-10-02-dataset-competition-design.md`.

I checked every cited test name with `grep -rl "fn <name>" crates scripts`.
All names exist. I added no test file.

Rows with a weak test pin:
- A §1: scope text has no behaviour. The row cites `arc_01_baseline_oracles_present`.
- B §1: scope text has no behaviour. The Spec A tests pin the subsumed refactor.

Legend and closing note of the file now say that the implemented, tested
behaviour is the spec and that no row is pending.

## req-coverage script

`scripts/check-req-coverage.sh` exits 1 for a reason outside T7. The untracked
file `ai_docs/designs/telemetry-and-balancing.md` duplicates every ID of
`ai_docs/designs/2026-09-28-fleet-telemetry-design.md`. The script reports
DUPLICATE for each ID (TEL, QUO, SPC, BAL, NFR-01..05). It reports no other
error. The owner of `telemetry-and-balancing.md` must rename or remove one file.
