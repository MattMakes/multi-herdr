# Second system review 39: control loop and crash safety

## Verdict

FAIL

0 HIGH findings remain open.

## Answers

- S39-1 (CRITICAL) closed: lines 285-286, 723-738 and 1256 move every listed gate, merge, rebase and network operation into a supervised job outside the pass, so a long gate no longer delays the heartbeat.
- S39-2 (CRITICAL) closed: lines 1215-1252 commit an intended row before each effect, observe existing attempt processes before repeating a launch, run stops before launches and never reuse an attempt id.
- S39-3 (HIGH) closed: lines 340, 806-813 and 817-819 require a lifetime OS lock, a generation check in every fleetd write transaction and lock handoff before replacement startup.
- S39-4 (HIGH) closed: lines 332-337 and 794-804 give each command a stable request id, payload hash, durable result, atomic application and replay, and archive its file only after commit.
- S39-5 (MEDIUM) closed: lines 740-747 and 1148 define a periodic fetch job, durable observed remote tips, stale-since reporting and fetched-tip use for merge and gate decisions.
- S39-6 (MEDIUM) closed: lines 811-812 and 1184-1190 define the fleet clock, cap its restart advance and assign each operational, awake-time and calendar timer to one clock.
- S39-7 decision.
- S39-8 (HIGH) closed: lines 179-181, 338 and 420-429 require peer-user checks, owner and mode checks, no-follow bounded file reads and refusal of operator commands inside fleet attempts.

## New findings

### T39-1 - CRITICAL - Worktree effects can still block the heartbeat and restart fleetd forever

Lines: 723-728, 1240-1245, 1256.

> `for e in effects:`
> `    do_effect(e)                # stop an orphan or an ended attempt; git worktree add or remove; write task.yaml + START.md;`

Failure scenario: Dispatch commits a `reserved` worktree row. Part 3 then runs `git worktree add` synchronously before lines 1244-1245 write the heartbeat and watchdog ping. The closed job-kind list at lines 723-728 has no worktree-create or worktree-remove job, although either checkout or removal can exceed 30 s. The watchdog kills fleetd, restart finds the same intended row, and the same blocking effect runs before the next heartbeat. The daemon can repeat this cycle forever, or observe and use a partial worktree left by the killed command.

Smallest change: Add worktree creation and removal to the durable job state machine, with an input key, time limit, validated completion and idempotent recovery. Do not run any potentially blocking effect in the heartbeat path.

### T39-2 - CRITICAL - Full fact scans bypass the pass budget

Lines: 1023, 1209-1214, 1258.

> `files = scan_outboxes()         # full scan, with the checks for an untrusted file; file watching only makes it run sooner`
> `trees = scan_worktrees()        # /fleet/work every pass, git worktree list every 60 s`

Failure scenario: D8 permits an unbounded count of workers, files and worktrees. Part 1 performs complete outbox and worktree scans before the transaction and before the cursor-based budget applies. Once either scan takes more than 30 s, fleetd reaches neither the heartbeat nor the persisted cursors. The watchdog restarts it, and each restart begins the same full scans from the start, so the control loop can stop for good.

Smallest change: Give fact acquisition durable cursors and per-pass item and time budgets, including directory and process scans. Move an expensive repository scan to a bounded job, and preserve enough scan progress that restart does not begin at item 1.
