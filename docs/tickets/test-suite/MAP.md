# Map: test-suite

Spec: `docs/specs/test-suite-audit.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [Merge PR #344 into the hark run branch](01-merge-344.md) |  | 1 |
| 02 | [U1: e2e harness skips the 5 s delivery probe, and stale couriers are stopped](02-u1-e2e-probe.md) | 01 | 2 |
| 03 | [U2: a fault point for the done push budget, with bounded-time assertions](03-u2-done-budget.md) | 01 | 2 |
| 04 | [U3: horch-core unit tests: 3 slow tests and 1 flaky test](04-u3-core-unit.md) | 01 | 2 |
| 05 | [U5: Rust tautology rewrites and duplicate removals](05-u5-rust-tautology.md) | 01 | 2 |
| 06 | [U6: hark Python tautologies](06-u6-hark-tautology.md) | 01 | 2 |
| 07 | [U7: skill-intake suite runs in parallel, and the gate runs the hark and one-page-design suites](07-u7-intake-parallel.md) | 01 | 2 |
| 08 | [U8: one-page-design Chrome probe](08-u8-chrome-probe.md) | 01 | 2 |

## Waves

- Wave 1: 01
- Wave 2: 02, 03, 04, 05, 06, 07, 08

## Critical path

01 -> 02

## Open questions

- Q1 (nextest) and Q5 (replace frozen oracles) go to the operator.
