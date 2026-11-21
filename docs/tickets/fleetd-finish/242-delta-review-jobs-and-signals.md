# Delta review (jobs-and-signals): the stop of a job, process groups, signal handlers and the sandbox of a job

**Persona:** primary `codex-sol`, fallback `opus-verify` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/reviews/delta-review-jobs-and-signals.md && grep -q '^## Findings' docs/specs/fleetd/reviews/delta-review-jobs-and-signals.md && grep -q '^## Verdict' docs/specs/fleetd/reviews/delta-review-jobs-and-signals.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/reviews/delta-review-jobs-and-signals.md`

## Why

Done criterion 4 of the run: an end review by a non-author, with each Critical and Important finding fixed or waived by the operator. The first end review (`docs/specs/fleetd/reviews/end-review-a.md`) read tip 08c6d4ff. Since then about 4300 lines of product code changed in a fix wave whose tasks had narrow Checks and no review (`**Review:** none`). You are the reviewer of 1 part of that change. You wrote none of it.

## Scope

Base: commit 50fe4705. Tip: the tip of `fleet/fleetd-finish` when you start (write it in the report). Read `git diff 50fe4705..HEAD -- crates/horch-platform/src crates/horch-core/src/fleetd/fleet_runtime crates/horch-core/src/fleetd/integration/adapters crates/horch-core/src/fleetd/worktree_custody/adapters crates/horch-marketplace/src` and the commit messages of `git log 50fe4705..HEAD -- crates/horch-platform/src crates/horch-core/src/fleetd/fleet_runtime crates/horch-core/src/fleetd/integration/adapters crates/horch-core/src/fleetd/worktree_custody/adapters crates/horch-marketplace/src`. Product code first; tests only to judge if they prove the claim of their commit.

## What to look for

The stop rules of a job (TERM first, the KILL after the grace of a recorded TERM, tasks T-319 and T-309); the own group and the registered kill of each job kind with sandboxed git; the notice of an orphan group and the wait of the git writer; the signal handler and its 8 slots (async-signal-safety, a lost or a double signal); bwrap --unshare-pid with an inner setsid for a job against --new-session for an agent (FR-10 reads host PIDs); the detached child on Windows that inherits no std handle (a static lock around the spawn: a deadlock or a handle that stays not inheritable after an error?); the launch window of a worker (SR-8: the wrapper is recorded before the agent starts); a failed spawn ends its attempt at once; the job host is the horch of the context and never current_exe; pid reuse in each place that sends a signal by pid or group id.

Also, for each commit: does the test fail without the fix (read it; do not trust the message)? Did a comment sweep remove a comment that gave a security rule, an invariant or a bound (compare with the commit before the sweep)? Does a change break a rule of `docs/specs/fleetd/design.md` (name the id: FR, RR, SR, OR, PL, FM, A)? Rules of this run that a change must keep: a pass never waits for a job (A23) and runs no SQL statement for each row; a row is written before its effect (A24); fleetd never stops a process that it did not start; no OS `cfg` outside `horch-platform` (PL-2); a worker never gets the operator token and sends no operator-class request (SR-8, FR-10).

## Output

`docs/specs/fleetd/reviews/delta-review-jobs-and-signals.md`: `## Scope` (base, tip, commits read); `## Findings`: each with an id (`D242-F1` ...), a class (Critical: a security rule or a correctness rule can break; Important: a design rule or a bound is not met, or a test does not prove its claim; Minor), the file and line, what breaks and how (a concrete order of events), and the smallest fix; `## Checked and good` (short); `## Verdict`. Label each finding "verified by reading" or "verified by a run" (you may run a test of 1 file through `scripts/dev-sandbox.sh cargo test -p <crate> --locked --test <file>`; at most 2 test processes at the same time; never a full suite). No finding without a file and line. Do not fix code.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. Never install the build or a tool.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never run pkill or killall. Never touch a process of another fleet. Write 1 `horch ledger progress` line each 10 minutes.
4. Before DONE: no process of the unit is alive, the sandbox folder and your `target/` are removed.

## Done when

The Check passes and each commit of the scope is named in the report as read.
