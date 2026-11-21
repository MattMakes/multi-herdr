# Delta review: jobs and signals

## Scope

- Base: `50fe4705`.
- Tip: `05ff408109f34d10dce7df94084c9b8979cb2cc9` (`fleet/fleetd-finish` at review worktree creation).
- Product paths read: `crates/horch-platform/src`, `crates/horch-core/src/fleetd/fleet_runtime`, `crates/horch-core/src/fleetd/integration/adapters`, `crates/horch-core/src/fleetd/worktree_custody/adapters`, and `crates/horch-marketplace/src`.
- Tests were read only when needed to judge a commit's claim. No test process was run; each finding below is verified by reading.

Commits read, oldest first:

1. `b0a932ec` — fleetd-finish 192: a cancel of a running task completes; the pass opens no I1 item for a task whose cancel waits (T-294).
2. `6710d16a` — comment sweep T-294: remove 18 narration comments from the cancel fix and its tests.
3. `ae08ee09` — fleetd-finish 157: no agent code runs before fleetd has recorded the process of its attempt (SR-8, FM-32, T-172).
4. `343d912f` — fleetd-finish 157: failing tests: a git writer that waits behind a running writer of its repository starts before newer writers (T-172).
5. `7c32d9d6` — fleetd-finish 157: the git writers of 1 repository start first in, first out, so a waiting rescue is not starved by newer fetches (T-172).
6. `43408dc8` — Comment sweep of T-172: remove 29 comments that restate code, private and test docs, and 1 banner (T-298).
7. `44923267` — fleetd-finish 157: restore 14 comments of the comment sweep that give a security rule, an invariant or a design id (SR-8, FM-32, T-172).
8. `1f919fea` — RR-13: fleetd refuses a drop whose tree HEAD moved during the checks (T-287).
9. `fb15c55f` — fleetd-finish R8-F3: retire the request kind spawn_adopt (T-302).
10. `2c6ef0b4` — T-302 comment sweep: remove 2 comments that retell code or park a retired statement.
11. `d8a08e35` — T-254 CI FIX-25: a worker_launch_failed report ends the push attempt of that worker crashed at once.
12. `ec95e902` — comment sweep T-254: remove 7 comments from the launch-failed fix and its tests.
13. `86d91196` — T-254: after the rebase on T-302, register reads deps again for worker_launch_failed.
14. `0069a171` — comment sweep T-254: remove 4 comments.
15. `6c453b24` — fleetd-finish 42 stage 2: the pass pages the I1 lease repairs and reads the personas of the workers in 1 statement (T-292).
16. `3f6321ff` — Comment sweep T-292: delete 36 narrating doc comments, name the start-of-pass persona read, put worker_personas below quota_hinted (T-316).
17. `b7ccafca` — fleetd-finish 42: restore the comments of the bounds and of the whole steps that the sweep removed (T-292).
18. `b29ac09b` — CI FIX-22: Linux job sandboxes get a PID namespace that a group KILL ends; 2 Linux test faults fixed (T-309).
19. `c9476163` — T-309 comment sweep: remove 11 comments that retell code or tests.
20. `d70a4032` — T-309: restore 3 comments of the sweep that give a security reason or an invariant, and name the FR-10 reason of host PIDs.
21. `497d9cf7` — fleetd-finish 168: worktree states landing and landed; the tree of landed work is removed at the land of its branch (T-286).
22. `c95240f8` — CI4 FIX-36: Windows: a verbatim path never reaches a child that start::Command starts (T-340).
23. `5b189db0` — Comment sweep T-348: remove 2 comments that retell code (T-349).
24. `84043e89` — T-348: restore the comment that says when a path keeps its verbatim prefix (T-340).
25. `b7c65960` — fleetd-finish 227: Windows: a detached child inherits no std handle of its parent (T-342).
26. `5efc8c11` — fleetd-finish 213: each context reads the tables of another context through the reader of the owner (T-327).
27. `f21c54e2` — fleetd-finish 228: a conformance check that fleetd cannot prove is "not proved" (T-347).
28. `16d454b9` — comment sweep T-327: delete 86 added comment lines on private items and test code (T-358).
29. `e9e07c76` — fleetd-finish 213: restore the comments of the sweep that give a snapshot invariant, a statement bound, a wait bound or a design rule (T-327).
30. `7a8e2a27` — fleetd-finish 208: each stop of a job ends its sandbox on Linux; TERM precedes KILL after the recorded grace (T-319).
31. `d63d174e` — Comment sweep T-319: remove 111 comment lines that narrate the code (T-367).
32. `9e039412` — T-319: restore the comments of the sweep that give a limit, a rule, or the fault that a test proves.

## Findings

### D242-F1 — Critical — an unrelated reused process group can receive TERM or KILL

**Verified by reading.** `crates/horch-platform/src/unix/group.rs:74` treats a group as the recorded job's group when the recorded leader PID no longer exists but any process currently has that group ID. `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:980` then sends TERM to each current member or KILL to the whole group.

Concrete order: the recorded job group ends completely; the OS reuses its numeric leader PID for an unrelated session or process-group leader; that new leader creates a child and exits; the child keeps the reused group ID; a delayed job stop calls `is_same_group`; `start_time(pgid)` is now `None`, while `kill(-pgid, 0)` succeeds; fleetd signals the unrelated child. This breaks the stated rule that fleetd never stops a process it did not start. The T-319 tests cover an original orphan group, but not reuse after the original group becomes empty.

Smallest fix: in the job-stop path, require the group leader to exist with the recorded start time. If the leader is gone, send no signal and use the existing `GROUP_LEFT` notice/hold path. Add a regression test that substitutes a leaderless group after the recorded group disappears.

### D242-F2 — Critical — TERM can arrive after a gate or check starts but before its group is armed

**Verified by reading.** `crates/horch-core/src/fleetd/integration/adapters/exec_gate.rs:478` starts the gate before line 479 stores its group. `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:1714` starts the check before lines 1716-1717 store and register its group.

Concrete order: `host.launch` creates the gate or check group; fleetd sends TERM to the job before the job thread stores the child PID in `GATE_GROUP` or `CHECK_GROUP`; the already-installed job signal handler swaps only zero-valued group slots, restores the default action, and ends the job; the child group receives no KILL and can run without a bound. This violates the T-319 claim that no gate or check outlives its stopped job. The tests wait until the child heartbeat is visible before they stop the job, so they cannot exercise this launch window.

Smallest fix: block the handled termination signals before launch, then store/register the child group and restore the signal mask. On each launch error, restore the mask before returning. Add a launcher test that delivers TERM in the post-spawn, pre-arm window.

### D242-F3 — Important — Windows silently continues when a standard handle cannot be made non-inheritable

**Verified by reading.** `crates/horch-platform/src/windows/group.rs:74-84` folds `GetHandleInformation` failure into `inherits = false`, ignores `SetHandleInformation` failure while clearing, and ignores every failure while restoring.

Concrete order: stdout is an inheritable pipe; `GetHandleInformation` or the clearing `SetHandleInformation` call fails; the handle is not added to `cleared`; `cmd.spawn()` still runs with handle inheritance enabled; the detached child inherits the pipe; its caller waits for EOF until that detached child exits, reproducing the hang T-342 was intended to prevent. A failed restore can also leave the parent handle's inherit flag changed after this function returns. The added test proves only the successful API path and has no fault injection for either failure.

Smallest fix: fail closed before spawn on any query or clear failure for a valid standard handle, restoring handles already changed. After spawn, report a restoration failure (and ensure every remaining handle is still attempted). Add injected Windows API tests for clear and restore failures.

### D242-F4 — Minor — the T-254 comment sweep removed the local RR-13 race invariant

**Verified by reading.** Commit `0069a171` removed the comment above `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs:311` and no later commit restores it. The comment documented why a commit made after the in-transaction HEAD check is not lost: rescue reads HEAD again, and removal accepts only the parent of the rescue commit.

Concrete order protected by the undocumented invariant: the drop checks HEAD; another writer commits; rescue must capture that newer HEAD; removal must reject any HEAD that is not the rescue parent. Removing the explanation does not change current execution, but it hides the cross-adapter RR-13 requirement from the check that appears locally redundant.

Smallest fix: restore the six-line invariant comment removed by `0069a171` above `refuse_moved_head`.

## Checked and good

- T-319 records the first TERM fact in the transaction before the signal effect (A24), reads the first fact for the current run/process in one statement, and derives KILL from the recorded TERM time. The domain tests would fail under the prior deadline-based immediate-KILL behavior.
- The Linux job sandbox uses `bwrap --unshare-pid` and an inner fixed-path `setsid`; host-PID sandboxes retain `--new-session`, preserving FR-10's host PID requirement.
- The job process registers at most `GIT_GROUP` plus `GATE_GROUP` or `CHECK_GROUP`. Re-registering one static does not consume another of the 8 slots, and the handler's atomic swap prevents a second handler invocation from killing the same recorded group.
- Sandboxed git arms `GIT_GROUP` only around the call. Each job kind that reaches the worktree git adapter first calls `kill_job_group_on_stop`.
- The pass builds job specifications from `self.settings.horch`, not `current_exe`; the synchronous custody adapter likewise receives its context's `horch` path.
- The wrapper writes its first heartbeat before agent launch and waits for its exact PID/start-time record before agent code runs. The failed-spawn handler ends a pending push attempt as `crashed` in the request transaction. The tests exercise both behaviors rather than only checking helper existence.
- The changed production code introduces no OS `cfg` outside `horch-platform`; the matches found in the scoped core and marketplace files are test-only.
- The T-172, T-292, T-309, T-327, T-340, and T-319 restoration commits put back the reviewed security rules, invariants, or bounds removed by their sweeps. The launch-failure rule removed locally by `ec95e902` remains documented on public `end_failed_spawn`.

## Verdict

Not ready. D242-F1 and D242-F2 can signal the wrong process or leave stopped job work alive. D242-F3 leaves the Windows hang fix open on ordinary API error paths. D242-F4 should restore the removed RR-13 invariant comment.
