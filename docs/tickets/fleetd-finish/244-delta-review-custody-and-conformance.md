# Delta review (custody-and-conformance): worktree custody, the reconciler, landings, the conformance verdicts and the question resume

**Persona:** primary `codex-sol`, fallback `opus-verify` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/reviews/delta-review-custody-and-conformance.md && grep -q '^## Findings' docs/specs/fleetd/reviews/delta-review-custody-and-conformance.md && grep -q '^## Verdict' docs/specs/fleetd/reviews/delta-review-custody-and-conformance.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/reviews/delta-review-custody-and-conformance.md`

## Why

Done criterion 4 of the run: an end review by a non-author, with each Critical and Important finding fixed or waived by the operator. The first end review (`docs/specs/fleetd/reviews/end-review-a.md`) read tip 08c6d4ff. Since then about 4300 lines of product code changed in a fix wave whose tasks had narrow Checks and no review (`**Review:** none`). You are the reviewer of 1 part of that change. You wrote none of it.

## Scope

Base: commit 50fe4705. Tip: the tip of `fleet/fleetd-finish` when you start (write it in the report). Read `git diff 50fe4705..HEAD -- crates/horch-core/src/fleetd/worktree_custody/application crates/horch-core/src/fleetd/worktree_custody/domain crates/horch-core/src/fleetd/worker_profiles crates/horch-core/src/fleetd/integration/application crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs crates/horch/src/cmd` and the commit messages of `git log 50fe4705..HEAD -- crates/horch-core/src/fleetd/worktree_custody/application crates/horch-core/src/fleetd/worktree_custody/domain crates/horch-core/src/fleetd/worker_profiles crates/horch-core/src/fleetd/integration/application crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs crates/horch/src/cmd`. Product code first; tests only to judge if they prove the claim of their commit.

## What to look for

The worktree states landing and landed (task T-286) against the state table of the design: each move, who makes it, in which transaction, and what a crash between 2 steps leaves. The conformance verdict 'not proved' (task T-347): can a CLI that never passed conformance get dispatch because its checks are 'not proved' and not 'failed'; is 'no channel server exists' decided in a way that a worker or a stale socket can fake; the unsupported agy makes no drift idea: can a real drift of a supported CLI be hidden by the same branch. The operator-boundary probe that compares the device and inode of the fleet root (task T-320): can ENOENT be read as a refusal when the path is only hidden by a mount, and what does Windows give. The resume of a session with the answer (RR-12, task T-300): what if the session is gone, and is an attempt burned. The key 'unlanded' of result.json (task T-288): can a worker use it to make the verifier accept less. The cancel that completes (T-294). If task T-289 landed when you read: the removal of a released gate tree and of a dropped scratch tree (rescue first, the keep-branch fact, the exclusion of a list job and a removal).

Also, for each commit: does the test fail without the fix (read it; do not trust the message)? Did a comment sweep remove a comment that gave a security rule, an invariant or a bound (compare with the commit before the sweep)? Does a change break a rule of `docs/specs/fleetd/design.md` (name the id: FR, RR, SR, OR, PL, FM, A)? Rules of this run that a change must keep: a pass never waits for a job (A23) and runs no SQL statement for each row; a row is written before its effect (A24); fleetd never stops a process that it did not start; no OS `cfg` outside `horch-platform` (PL-2); a worker never gets the operator token and sends no operator-class request (SR-8, FR-10).

## Output

`docs/specs/fleetd/reviews/delta-review-custody-and-conformance.md`: `## Scope` (base, tip, commits read); `## Findings`: each with an id (`D244-F1` ...), a class (Critical: a security rule or a correctness rule can break; Important: a design rule or a bound is not met, or a test does not prove its claim; Minor), the file and line, what breaks and how (a concrete order of events), and the smallest fix; `## Checked and good` (short); `## Verdict`. Label each finding "verified by reading" or "verified by a run" (you may run a test of 1 file through `scripts/dev-sandbox.sh cargo test -p <crate> --locked --test <file>`; at most 2 test processes at the same time; never a full suite). No finding without a file and line. Do not fix code.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. Never install the build or a tool.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never run pkill or killall. Never touch a process of another fleet. Write 1 `horch ledger progress` line each 10 minutes.
4. Before DONE: no process of the unit is alive, the sandbox folder and your `target/` are removed.

## Done when

The Check passes and each commit of the scope is named in the report as read.
