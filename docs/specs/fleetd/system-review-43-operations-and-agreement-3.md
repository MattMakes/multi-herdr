# Third system review 43: operations, trust, and document agreement

## Verdict

PASS

Open HIGH findings: T43-2.

## Answers

- T43-1 (CRITICAL) closed: lines 733-751, 1311-1312, 1388, and 1615 give `docs_sync` and `conformance` valid job kinds, nullable repository bindings, subjects, input keys, run numbers, time limits, and Phase 7 row tests.
- T43-2 (HIGH) open: at the authorization step, lines 345 and 909 allow a `no sandbox` worker to run with the user's rights and read the operator token; lines 342 and 344 require the token for a request file but apply the process-origin check only to the live channel, so that worker can write an authorized operator request file. Line 1437 also says a sandboxed persona does not start without a sandbox, which contradicts line 909.
- T43-3 (HIGH) closed: lines 741-753, 1101, 1183, and 1295-1312 make push and fetch effect jobs whose results are never reused, give each need a new run number, and schedule new runs after push failures and fetch periods.

## New CRITICAL

None
