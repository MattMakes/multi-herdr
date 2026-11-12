# Fleetd design review, revision round 2

## Verdict

FAIL

## Answers

- A19 stated (design lines 45, 78, 176, 180, 267, 293, 1258-1277 and 1293).
- A20 stated (design lines 654, 660-673, 770, 916, 930 and 1008).
- A21 stated (design line 1116).

## Findings

### L1 - CRITICAL - Phase P cannot pass its own gate

Line: `docs/specs/fleetd/design.md:180`, `docs/specs/fleetd/design.md:1258-1259`

> "The test suite passes on macOS, Linux and Windows before the move and after it"
> "Today most should fail."

Phase 0 deliberately leaves red tests for behavior implemented in Phases 1 to 7, so Phase P cannot make PL-6 green with only Phases 0 and P.

### L2 - LOW - The decision count is stale

Line: `docs/specs/fleetd/design.md:27`

> "A design review on 2026-11-11 and 2026-11-12 made 13 decisions."

The introduction still says 13 decisions after D16 and the revision-round row were added, so its count is stale.
