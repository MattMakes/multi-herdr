# fleetd amended design review

Scope: `docs/specs/fleetd/design.md` at `0505712d`, compared with amendments A0 to A14, the decisions file, and the source copy at `3ccf9da8`.

## Verdict

FAIL — CRITICAL: 1; HIGH: 3; MEDIUM: 0; LOW: 0.

## CRITICAL

### J1 — A0, A1 — `docs/specs/fleetd/design.md:15`

> A missed event is caught on the next pass, and a hung daemon is restarted.

This contradicts lines 292 and 1200, which leave hang detection unsupported on macOS and Windows, so the summary promises first-class restart behavior that the design does not build.

Correction: Define hang detection through the platform layer for all 3 hosts, or qualify the summary and requirements consistently.

## HIGH

### J2 — A3, A6 — `docs/specs/fleetd/design.md:738`

> with no `--to` it starts a new feature whose branch name fleetd makes from the ticket title.

Line 859 instead says the sub-orchestrator supplies the name and fleetd creates the branch when assumptions are first written, but `--direct` skips the assumptions phase.

Correction: Give a `--direct` feature one branch-name source and one creation trigger in both sections.

### J3 — A3, A4 — `docs/specs/fleetd/design.md:139`

> `horch idea add --direct <ticket file>` shall ... make a plan with 1 task

Lines 131 and 267 require every judgment task to have a paired judge task, so the document gives no valid plan shape when a direct ticket is a judgment kind.

Correction: State whether `--direct` excludes judgment kinds or may create the required second judge task.

### J4 — A4 — `docs/specs/fleetd/design.md:426`

> kind: report
> judges: retry-doc

The example makes the judge a judgment task, while lines 131 and 267 require every judgment task to have its own paired judge, so literal validation either rejects this plan or recurses without end.

Correction: Define judge tasks as an explicit terminal exception to the pairing rule.

## MEDIUM

No MEDIUM findings.

## LOW

No LOW findings.

Amendment coverage:

- A0: J1.
- A1: J1.
- A2: no finding.
- A3: J2, J3.
- A4: J3, J4.
- A5: no finding.
- A6: J2.
- A7: no finding.
- A8: no finding.
- A9: no finding.
- A10: no finding.
- A11: no finding.
- A12: no finding.
- A13: no finding.
- A14: no finding.
