# Systems review 42, round 3: migration plan and requirements

Scope: `docs/specs/fleetd/design.md` at `HEAD`, with the change since `c8bf35ac` checked for new CRITICAL faults.

## Verdict

PASS

Open HIGH findings: S42-2, T42-1.

## Answers

- S42-2 (HIGH) open: At the architecture-to-migration consistency step, `design.md:1568` still says there is a single writer and every other process opens the ledger read-only, while `design.md:578`, `1601`, and `1657-1660` require worker and `horch` commands to write directly in Phases 1 to 4.
- S42-4 (HIGH) closed: `design.md:1553`, `1612`, `1614`, and `1644-1646` make the Phase 4 check only the code-kind and structural parts, and make RR-3 and full FM-9 green in Phase 6 with the judge pair.
- T42-1 (HIGH) open: At the raw request-file step, `design.md:909` accepts a no-sandbox profile, and `design.md:345` says its worker has the user's rights and can read the token, so the owner, mode, and token checks at `design.md:340-342` pass; the process check at `design.md:344` applies only to the live channel.

## New CRITICAL

None
