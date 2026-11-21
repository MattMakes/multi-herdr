# Delta review (ledger-reads-and-locks): set readers of a pass, snapshots, the read-only ledger and the folder lock

**Persona:** primary `codex-sol`, fallback `opus-verify` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/reviews/delta-review-ledger-reads-and-locks.md && grep -q '^## Findings' docs/specs/fleetd/reviews/delta-review-ledger-reads-and-locks.md && grep -q '^## Verdict' docs/specs/fleetd/reviews/delta-review-ledger-reads-and-locks.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/reviews/delta-review-ledger-reads-and-locks.md`

## Why

Done criterion 4 of the run: an end review by a non-author, with each Critical and Important finding fixed or waived by the operator. The first end review (`docs/specs/fleetd/reviews/end-review-a.md`) read tip 08c6d4ff. Since then about 4300 lines of product code changed in a fix wave whose tasks had narrow Checks and no review (`**Review:** none`). You are the reviewer of 1 part of that change. You wrote none of it.

## Scope

Base: commit 50fe4705. Tip: the tip of `fleet/fleetd-finish` when you start (write it in the report). Read `git diff 50fe4705..HEAD -- crates/horch-core/src/taskledger crates/horch-core/src/fsx.rs crates/horch-core/src/fleetd/task_execution crates/horch-core/src/fleetd/ideas crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs crates/horch-core/src/fleetd/fleet_runtime/application/dispatch.rs crates/horch/src/cmd/taskledger.rs` and the commit messages of `git log 50fe4705..HEAD -- crates/horch-core/src/taskledger crates/horch-core/src/fsx.rs crates/horch-core/src/fleetd/task_execution crates/horch-core/src/fleetd/ideas crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs crates/horch-core/src/fleetd/fleet_runtime/application/dispatch.rs crates/horch/src/cmd/taskledger.rs`. Product code first; tests only to judge if they prove the claim of their commit.

## What to look for

The paged and set-based readers of a pass (tasks T-292, T-313, T-327): does each multi-statement read run on 1 snapshot, and can a write between 2 statements still give a torn result; does a page miss or repeat a row when rows change between pages; is the result the same as the old per-row code for each edge (NULL, no row, 2 rows with the same key, the order); does a reader of another context go through the reader of the owner. The read-only ledger of horch ledger status (the trace hook counts only the savepoint one_snapshot as a read: can a write hide behind it); the record of the last statement that belongs to its connection (an unsafe trace callback with a context pointer: the life of the record against the life of the connection, also when the Store is moved or the connection is taken out of it). The folder lock of fsx.rs (task T-368): the take, the release under the breaker guard, the break with a tomb and an owner check: find an order of events with 2 holders on Unix with a working guard; judge the named limits. The base tips read before the write transaction (T-291): can a tip change between the read and the use, and what then.

Also, for each commit: does the test fail without the fix (read it; do not trust the message)? Did a comment sweep remove a comment that gave a security rule, an invariant or a bound (compare with the commit before the sweep)? Does a change break a rule of `docs/specs/fleetd/design.md` (name the id: FR, RR, SR, OR, PL, FM, A)? Rules of this run that a change must keep: a pass never waits for a job (A23) and runs no SQL statement for each row; a row is written before its effect (A24); fleetd never stops a process that it did not start; no OS `cfg` outside `horch-platform` (PL-2); a worker never gets the operator token and sends no operator-class request (SR-8, FR-10).

## Output

`docs/specs/fleetd/reviews/delta-review-ledger-reads-and-locks.md`: `## Scope` (base, tip, commits read); `## Findings`: each with an id (`D243-F1` ...), a class (Critical: a security rule or a correctness rule can break; Important: a design rule or a bound is not met, or a test does not prove its claim; Minor), the file and line, what breaks and how (a concrete order of events), and the smallest fix; `## Checked and good` (short); `## Verdict`. Label each finding "verified by reading" or "verified by a run" (you may run a test of 1 file through `scripts/dev-sandbox.sh cargo test -p <crate> --locked --test <file>`; at most 2 test processes at the same time; never a full suite). No finding without a file and line. Do not fix code.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. Never install the build or a tool.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never run pkill or killall. Never touch a process of another fleet. Write 1 `horch ledger progress` line each 10 minutes.
4. Before DONE: no process of the unit is alive, the sandbox folder and your `target/` are removed.

## Done when

The Check passes and each commit of the scope is named in the report as read.
