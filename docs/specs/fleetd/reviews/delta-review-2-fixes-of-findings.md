# Delta review 2: fixes of findings

## Scope

- Base: `25660806`.
- Tip: `3d3e9a8ad45f607701b8cedb532b2881f5c0ac62` (`fleet/fleetd-finish` at worktree creation).
- I read the product diffs in `crates/horch-platform/src`, `crates/horch-marketplace/src/git.rs`, `crates/horch-core/src/fsx.rs`, the changed fleet runtime, integration, task-execution and worktree-custody modules, `crates/horch/src/cmd`, and the changed e2e harness. I read tests only to judge the claims. I did not run the test wrapper, as required by the ticket.
- T-384, T-289 and T-385 have no commit in `25660806..3d3e9a8a`; I read none of their unlanded branch commits as part of this tip.

Commits read, oldest first:

1. `269cae95` — T-377: Linux proof of each test that needs git 2.41.
2. `c67ba24f` — T-383: fail closed for Windows detached standard handles; restore the RR-13 comment.
3. `64a680f6` — T-387: comment sweep of T-383.
4. `621c1960` — T-383: restore the fail-closed documentation.
5. `249faec1` — T-382: a release without the breaker guard marks its lock and removes nothing.
6. `9e17b608` — T-388: comment sweep of T-382.
7. `72b48f3e` — T-382: restore the folder-lock invariants.
8. `1eddd07e` — T-390: operator decisions.
9. `7d08119a` — T-369: hold termination across child launch and group publication.
10. `65e6f563` — T-369: put a 10 s limit on each git of fleetd.
11. `f6482589` — T-369: report `.lock` files left by a timed-out git.
12. `bf60e496` — T-369: compile fix for the stop-window test.
13. `af81a088` — T-369: preserve project-file read and parse failures.
14. `dbf50db6` — T-369: require the recorded live leader before a job-group stop.
15. `76ace426` — T-369: apply the leader rule to task-attempt stops and document FM-22.
16. `edf19799` — T-369: apply the leader rule to e2e teardown.
17. `6702b94f` — T-369: accept the Linux lock-test stop text.
18. `7ac2c2a8` — T-392: comment sweep of T-369.
19. `8847bb7a` — T-391: update the pull-request draft.
20. `513b73e1` — T-389: check the rescue git version in doctor and before fleetd starts.
21. `92beba7a` — T-393: comment sweep of T-389.
22. `9d8e3bdc` — T-389: restore the git-selection and pre-row check rules.
23. `dc93558a` — T-386: record `resume_refused` and launch fresh without burning an attempt.
24. `25aaa3de` — comment sweep of T-386.
25. `3d3e9a8a` — T-386: restore the attempt-floor, retry-budget and CLI-wait comments.

## Findings

### D254-F1 — Critical — the live-leader check and group kill are not one operation

**Verified by reading.** Files: `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:1010-1023`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs:85-102`, and `crates/horch-e2e/src/harness.rs:757-763` and `818-827`.

Concrete order: fleetd verifies that PID and process-group ID 400 lead the recorded group with the recorded start; fleetd is descheduled; the leader and every member end, so ID 400 becomes free; an unrelated process with PID 400 creates a group and a child; that leader can also end; fleetd resumes. On the KILL path it sends `SIGKILL` to group 400. On the TERM path it enumerates the reused group's members, reads their current start times, and signals those unrelated processes through `stop_same`. The e2e harness has an even larger interval because it saves the verified group IDs, then sends TERM and later KILL. The new comments name reuse only before the check or after a leader has already gone. They do not name reuse between the check and the member snapshot or group signal. This still lets fleetd stop a process that it did not start.

Smallest fix: for `LeaderGroup`, snapshot each member and its start time, then recheck that the recorded leader still leads with its recorded start, then signal only the snapshot through `stop_same` for both TERM and KILL. Do not call `kill_group_id` after a separate liveness check. Apply the same snapshot, final leader check and per-process identity check in the harness. This can safely miss a concurrently created child, but it cannot hit a reused group.

Exact proving test command after adding a deterministic pause between the check and effect: `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test worktree_jobs_stop a_reused_group_after_the_leader_check_gets_no_kill` and `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked reused_group_after_teardown_check_gets_no_signal`.

### D254-F2 — Important — an interrupted non-reaping wait abandons the timed git

**Verified by reading.** Files: `crates/horch-marketplace/src/git.rs:479-503`, `crates/horch-marketplace/src/git.rs:511-525`, and `crates/horch-platform/src/unix/group.rs:43-58`.

Concrete order: the limited runner spawns git and starts its pipe readers; `waitid(..., WNOHANG | WNOWAIT)` is interrupted by a caught signal and returns `EINTR`; the `?` at line 482, or the one reached through line 516 during the TERM grace, returns from `output_within`; `Child` drops without a wait, the Unix `Contained` value has no terminating drop, and the reader threads detach. The git group can keep running, keep a `.lock`, and outlive fleetd's call. The promised 10 s TERM/KILL bound and the claim that every child is reaped therefore do not hold on this error path.

Smallest fix: retry `waitid` on `ErrorKind::Interrupted`. Also use a cleanup guard after spawn that kills the contained group and reaps the child on every other early return, then disarm it only after the explicit wait.

Exact proving test command after adding a wait seam that returns `Interrupted` once: `scripts/dev-sandbox.sh cargo test -p horch-marketplace --locked git::tests::an_interrupted_wait_still_stops_and_reaps_the_limited_git`.

### D254-F3 — Important — a restart can lose every notice of a lock left by the timed git

**Verified by reading.** Files: `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:768-818` and `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:317-321` and `550-581`.

Concrete order: a git in integration exceeds its limit; KILL leaves a `.lock`; `locks_left` stores its path only in the process-local `LOCKS_LEFT` set; the current pass has already run `jobs::observe`, which is the only caller that opens the needs-you item; fleetd exits or crashes before the next pass; the restarted process has an empty set, so it never opens or resolves the promised item. Later git calls can keep failing with the path in their pass error, but the required durable needs-you item is absent.

Smallest fix: persist each discovered path in a fleet-owned journal before returning the timeout error. At startup and in `jobs::observe`, ingest that journal idempotently into needs-you rows and retain each entry until its file is gone. Do not rely on process memory for the handoff between passes.

Exact proving test command after adding a restart between the timeout pass and the observe pass: `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test fleetd_pass_p4b a_git_lock_notice_survives_a_fleetd_restart`.

Prior-review status, one line for each finding:

- D242-F1 — fixed with a new fault, D254-F1; the code rejects a leaderless recorded group, but the verified live group can be reused before member enumeration or `killpg` (`crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:1010-1023`).
- D242-F2 — fixed; the handler is installed before launch and the launch-to-store interval is held (`crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:1745-1755` and `crates/horch-core/src/fleetd/integration/adapters/exec_gate.rs:477-487`).
- D242-F3 — fixed; a valid standard handle query or clear failure spawns nothing, and restoration attempts every cleared handle (`crates/horch-platform/src/windows/group.rs:108-158`).
- D242-F4 — fixed; the RR-13 cross-adapter invariant is restored (`crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs:311-316`).
- D243-F1 — fixed; a release without the breaker guard removes nothing, and the marked owner is stale under the next guarded break (`crates/horch-core/src/fsx.rs:404-443` and `662-683`).
- D244-F1 — not fixed in this tip; T-384 did not land, and `NotProved` still writes no dispatch hold (`crates/horch-core/src/fleetd/worker_profiles/application/health.rs:75-85`).
- D244-F2 — not fixed in this tip; active-tree reconciliation still calls 3 SQL readers for each row (`crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs:504-525` and `569-578`).
- D244-F3 — fixed; `resume_refused` is durable in the attempt row, advances the attempt floor once, and the next attempt is fresh (`crates/horch-core/src/fleetd/task_execution/application/launch.rs:438-519`).

## Checked and good

- The termination hold stores the hold count and first signal in one atomic word. The handler and last drop use compare-and-swap, nested holds keep the signal, and RAII ends the hold on early return or unwind. The actual gate and check call sites register their static group before opening the hold. The handler path uses atomics, `killpg`, `signal` and `raise`, restores the received signal's default action, and raises it again. Windows explicitly documents and implements the hold as a no-op.
- D242-F2's launch window is closed in both call sites. A launch error drops the hold before returning. A panic also drops it during unwind. Two threads that hold concurrently share the counter, and only the last drop acts on the stored signal.
- T-382's four-step proof holds with a working Unix breaker guard. Every current remover has the guard; publishing fills only an empty path; a release that times out only writes `released-<nonce>`; and `stale_lock` accepts a release mark only when it names the current owner's nonce. A late mark in another holder's directory is inert. A nonce collision is already outside the lock's UUID identity assumption. Marks stay inside the lock directory and leave when that lock is removed, so they do not create a separate accumulating namespace. The documented Windows and `flock`-error put-back limit is accurate.
- On the normal Git timeout path the child remains unreaped until TERM and KILL finish, so its group ID cannot be reused before the kill. The child is then waited and both pipe readers are joined. A normal needs-you item can name a pre-existing lock held by another git because the scan reads all writable git directories, but its text requires the operator to verify that no git runs before removal; fleetd never deletes it.
- A failed integration duty rolls back only that duty's SQL savepoint. Files and refs already changed by earlier git commands are external effects and do not roll back; later reconciliation observes them. The reviewed timeout test covers the trial-merge case, but not every duty. No pass waits for a queued job.
- `resume_refused` can occur at most once after each answered `asked` attempt because the next attempt follows the durable `resume_refused` row, not `asked`. Fast exit alone does not repeat the free launch. `attempt_floor += 1` preserves `attempts - attempt_floor`, which is the value used by retry backoff, failure, escalation and attempt-history reporting. A new answered question can authorize one new fresh fallback, as RR-12 requires.
- The Windows detached spawn fails before spawn on query or clear failure, restores all handles it previously cleared, and returns the child after a post-spawn restore failure so the caller does not lose ownership.
- `rescue_git_version` fails closed on missing prefixes, missing major or minor components, non-numeric components, old Apple and Windows forms, and command failure. It accepts suffixes after the parsed minor component, including current Apple and Windows version text. Fleetd performs the check before opening its writer, writing its fleetd row, or queuing a job. Doctor checks the same configured binary and rule.
- T-384 is not in this tip, so the unproved-conformance dispatch hold was not reviewed as landed work. The existing D244-F1 remains open rather than being silently treated as fixed.
- The changed non-test production code adds no OS `cfg` outside `horch-platform`. The reviewed paths preserve SR-8 and FR-10 ordering and identity rules except for the explicit findings above.

## Verdict

Not ready. This delta adds 1 new Critical fault and 2 new Important faults. D254-F1 can kill an unrelated reused process group. D254-F2 can leave a timed git running and unreaped. D254-F3 can lose the required lock notice across restart. The earlier Critical D244-F1 and Important D244-F2 also remain unfixed because their fixes are not in this tip.
