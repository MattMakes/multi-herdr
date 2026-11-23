# Delta review: ledger reads and folder locks

## Scope

- Base: `50fe4705`.
- Tip reviewed: `5ab3293ca92f430cdc7a817295d37709b24dbc48`.
- Product code was read before tests. Tests were read to judge whether they prove each commit's claim.
- The optional focused test runs were unavailable because `scripts/dev-sandbox.sh` could not create its `/tmp/hds.*` directory in this sandbox. No finding is labeled as run-verified.

Commits read, in order:

- `b0a932ec` — fleetd-finish 192: a cancel of a running task completes; the pass opens no I1 item for a task whose cancel waits (T-294)
- `6710d16a` — comment sweep T-294: remove 18 narration comments from the cancel fix and its tests
- `455f0881` — fleetd-finish 157: failing tests: no agent code runs before fleetd records its wrapper (SR-8, FM-32, T-172)
- `ae08ee09` — fleetd-finish 157: no agent code runs before fleetd has recorded the process of its attempt (SR-8, FM-32, T-172)
- `343d912f` — fleetd-finish 157: failing tests: a git writer that waits behind a running writer of its repository starts before newer writers (T-172)
- `7c32d9d6` — fleetd-finish 157: the git writers of 1 repository start first in, first out, so a waiting rescue is not starved by newer fetches (T-172)
- `43408dc8` — comment sweep of T-172: remove 29 comments that restate code, private and test docs, and 1 banner (T-298)
- `44923267` — fleetd-finish 157: restore 14 comments of the comment sweep that give a security rule, an invariant or a design id (SR-8, FM-32, T-172)
- `d8a08e35` — T-254 CI FIX-25: a `worker_launch_failed` report ends the push attempt crashed at once
- `ec95e902` — comment sweep T-254: remove 7 comments from the launch-failed fix and its tests
- `6d9e4a1d` — fleetd-finish 42 stage 1: `sync_native` reads `import_findings` 1 time per statement (T-292)
- `ddceb62c` — fleetd-finish 42 stage 1: `sqlite::rows` reads the tasks of the live attempts in 1 statement (T-292)
- `650da1b5` — fleetd-finish 42 stage 1: `lease::check_i1_within` repairs at most a budget of leases (T-292)
- `6c453b24` — fleetd-finish 42 stage 2: the pass pages the I1 lease repairs and reads worker personas in 1 statement (T-292)
- `7d8096f9` — fleetd-finish 42 stage 2: `dispatch::choices` reads running tasks and their Own files in 2 statements (T-292)
- `cc0d7e23` — fleetd-finish 42 stage 2: the sub-orchestrator need reads a failed task of an idea in 1 statement (T-292)
- `3f6321ff` — comment sweep T-292: delete 36 narrating doc comments, name the start-of-pass persona read, and move `worker_personas`
- `b7ccafca` — fleetd-finish 42: restore the comments of the bounds and whole steps removed by the sweep (T-292)
- `583b4402` — fleetd-finish 199: an answered question resumes the worker session (RR-12, R8-F1, T-300)
- `90a1fa49` — comment sweep T-300: delete 20 comments that retell code or test names (T-317)
- `9da64804` — fleetd-finish 199: restore 7 comments that give a security rule, design id, or reason (RR-12, T-254, T-300)
- `72f78a71` — T-307: a failed launch host call ends its push attempt crashed in the same pass (T-254)
- `0b81e22e` — fleetd-finish 206: `dispatch::choices` and `read_base_tips` read ready tasks in a fixed number of statements (T-313)
- `571d5000` — fleetd-finish 206: comment sweep of the T-313 diff removes 16 comments (T-313)
- `895e3848` — fleetd-finish 206: restore 2 comments of the sweep on the ready rows reader (T-313)
- `f82ac305` — CI4 FIX-26: the docs job names private `verifier::park_unlanded` in code font (T-331)
- `c95240f8` — CI4 FIX-36: a Windows verbatim path never reaches a child command (T-340)
- `f18eb76e` — Windows: test code that starts git gets a plain path from `for_child` (T-355)
- `a5987157` — fleetd-finish 213: multi-statement set readers of dispatch and reap rows read 1 snapshot (T-327)
- `5efc8c11` — fleetd-finish 213: each context reads another context's tables through the owner's reader (T-327)
- `f21c54e2` — fleetd-finish 228: an unprovable conformance check is not proved; `agy` is unsupported (T-347)
- `10584db0` — fleetd-finish 213: the read-only CLI SQL trace counts the `one_snapshot` savepoint as a read (T-327)
- `d025b4c0` — fleetd-finish 213: a test pins that the trace counts only the `one_snapshot` savepoint as a read (T-327)
- `16d454b9` — comment sweep T-327: delete 86 added comment lines on private items and test code (T-358)
- `e9e07c76` — fleetd-finish 213: restore comments that give a snapshot invariant, statement bound, wait bound, or design rule (T-327)
- `8235f257` — fleetd-finish 233: the last-statement record belongs to its read-only `Store` (T-359)
- `59fbae13` — fleetd-finish 233: the comment sweep removes the `Store` field-order comment (T-359)
- `71ad6e5c` — fleetd-finish 233: restore the `Store` field-order comment (T-359)
- `2dda73a9` — fleetd-finish 237: a lock break removes only the lock judged stale; a release holds the breaker guard (T-368)
- `bfe98185` — T-368 comment sweep: remove 9 comments on lock release and break
- `5ab3293c` — T-368: restore the lock-fix reasons and add the Limits documentation at `remove_lock`

## Findings

### D243-F1 — Critical — verified by reading

File: `crates/horch-core/src/fsx.rs:404` and `crates/horch-core/src/fsx.rs:428`.

`DirLockGuard::drop` continues after `wait_for_guard` returns `None` at 5 seconds. The nonce check does not preserve mutual exclusion because `remove_lock` temporarily removes the public lock pathname before it can put a rejected tomb back.

A concrete Unix order with a working `flock` guard is:

1. Holder A's lock becomes stale. Breaker P takes the breaker guard, judges A stale, and is descheduled for more than 5 seconds.
2. A drops. Its release times out waiting for P's guard and continues without it.
3. A's release verifies A's nonce and renames A's lock to its tomb.
4. Holder B publishes at the now-empty lock pathname and returns from `acquire`.
5. P resumes and renames B's lock to P's tomb. P rejects B's nonce because it judged A.
6. Before P restores B's tomb, holder C publishes at the empty pathname and returns from `acquire`.
7. P's no-replace restore fails and P deletes B's tomb. B and C both hold returned `DirLockGuard` values and can run protected work concurrently.

The Limits comment at `crates/horch-core/src/fsx.rs:446` accurately names this outcome, but documenting it does not preserve the cross-process lock's mutual-exclusion contract. The new test waits only 500 ms inside the breaker hook, so it proves the normal guarded ordering and cannot reach the 5-second fallback.

Smallest fix: when `wait_for_guard` returns `None`, make `DirLockGuard::drop` return without removing the lock. The active breaker can finish its stale-lock decision; a later acquirer can clean a lock left by the timed-out release. Add a deterministic hook test that drives the timeout path and proves that a third publisher cannot overlap the rejected holder.

Suggested post-fix test command: `scripts/dev-sandbox.sh cargo test -p horch-core --locked --lib fsx::tests::dirlock_release_timeout_does_not_overlap_a_new_holder` after adding that named regression test.

## Checked and good

- Verified by reading: `ReadyRows`, `running_tasks`, dispatch choices, and reap rows place each related multi-statement read inside `one_snapshot`. Nested savepoints retain the caller transaction's snapshot.
- Verified by reading: the set readers preserve absence and `NULL` behavior. Primary keys make duplicate map keys impossible; Own-file ordering remains by path; `newest_of_tasks` uses the same newest non-removed row as `of_task`.
- Verified by reading: a commit between the first and second ready-row statements cannot tear the result. The race test commits exactly in that window and compares task membership with Own-file membership.
- Verified by reading: bounded I1 repair repeats only unresolved rows. A successful repair disappears from the next pass's repair set. The bound assumes no new breach is introduced between passes.
- Verified by reading: `read_base_tips` may become stale before the write transaction, but the recorded worktree row names the exact commit read. A newly ready task without a cached tip fails its bind and retries in a later pass; it does not use another task's tip.
- Verified by reading: cross-context reads added in T-327 go through owner adapters, including worker personas, idea states, worktrees, runs, questions, and attempt outcomes.
- Verified by reading: `one_snapshot`'s savepoint is exempted from the debug write trace, while statements inside it still receive their own authorizer actions. The test includes an attempted insert inside the savepoint.
- Verified by reading: `ReadOnlyRecord` owns a heap-stable `Arc` allocation. `Store` drops `conn` before `record`, so SQLite cannot call the context pointer after the final owning field is dropped; moving `Store` does not move the callback target.
- Verified by reading: the comment sweeps restored the security rules, snapshot invariants, statement bounds, and unsafe drop-order invariant identified by their follow-up commits.
- Verified by reading: the reviewed changes keep A23/A32's pass bounds, A24's row-before-effect ordering, PL-2's platform boundary, and SR-8/FR-10's wrapper-before-agent and operator-token rules.
- The T-292, T-313, T-327, T-359, and T-368 tests were read against their pre-fix implementations. The lock race test fails without the normal-order fix, but it does not prove the documented 5-second fallback safe.

## Verdict

Not approved. D243-F1 is a Critical correctness finding: the folder lock can return two live holders on Unix even when `flock` works. The ledger snapshot, set-reader, read-only trace, and callback-lifetime changes are otherwise approved by reading.
