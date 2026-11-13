# Third system review 39: control loop and crash safety

## Verdict

PASS

No HIGH finding is open.

## Answers

- T39-1 (CRITICAL) closed: lines 1030-1039 make worktree creation and removal wait for durable jobs; lines 1276-1282 keep those git commands out of the heartbeat path; lines 1293 and 1308-1310 require working-tree commands to be jobs and define bounded, idempotent recovery.
- T39-2 (CRITICAL) closed: lines 764-769 persist cursors for the fact scans; lines 1244-1250 budget each scan before the transaction; lines 1314-1318 require per-scan and whole-pass time budgets, continue at each cursor, and preserve the heartbeat.

## New CRITICAL

None
