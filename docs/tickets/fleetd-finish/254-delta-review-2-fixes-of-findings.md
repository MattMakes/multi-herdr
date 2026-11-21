# Delta review 2: the fixes of the review findings (signals, the folder lock, the git limit, the resume, the git version check)

**Persona:** primary `codex-sol`, fallback `opus-verify` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/reviews/delta-review-2-fixes-of-findings.md && grep -q '^## Findings' docs/specs/fleetd/reviews/delta-review-2-fixes-of-findings.md && grep -q '^## Verdict' docs/specs/fleetd/reviews/delta-review-2-fixes-of-findings.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/reviews/delta-review-2-fixes-of-findings.md`

## Why

3 delta reviews (`docs/specs/fleetd/reviews/delta-review-*.md`) found 4 Critical and 3 Important faults. Fix tasks landed since, again with narrow Checks and no review. A fix of a concurrency or a signal fault can be wrong in a new way. You review the fixes. You wrote none of them.

## Scope

Base: commit 25660806. Tip: the tip of `fleet/fleetd-finish` when you start (write it in the report). `git log --reverse --format='%h %s' 25660806..HEAD` and the diff of each product file. Tasks in the range: T-369 (10 commits), T-382, T-383, T-386, T-389, and what landed of T-384, T-289, T-385 when you read (name what you read).

## What to look for

1. The hold of the termination handler (`horch-platform` `signal.rs`, `unix/signal.rs`; task T-369, finding D242-F2): a counter of holds and the first held signal in 1 atomic; the handler does a compare-and-swap. Find an order of events with: a signal that is lost; a signal that acts 2 times; a hold that never ends (a panic, an early return, 2 threads that hold); a group that is registered after the hold ended; work in the handler that is not async-signal-safe; the default action that is not restored, so the job process does not end. Is the Windows form (the hold does nothing) named as a limit?
2. A signal only for a group whose leader runs with the recorded start (D242-F1; `is_group_leader`): each stop path of a job, of a task attempt and of the e2e harness. Is there a window between the check and the kill (the leader ends and the id is reused in between)? How large, and is it named? The FM-22 stop of an orphan group keeps the hole by design: is its "Limit" comment true?
3. The folder lock (`crates/horch-core/src/fsx.rs`; T-368 and T-382, finding D243-F1): a release without the guard writes a mark `released-<nonce>` and removes nothing; `stale_lock` reads a marked lock as stale. Find an order of events with 2 holders on Unix with a working guard; a lock that no taker can ever break; a mark that makes the lock of a live holder stale (a nonce that repeats? a mark written into a lock of the same nonce after a re-take?); a leak of mark files. Read the proof in the commit message of T-382 and test each step of it against the code.
4. The time limit of a git of the fleetd process (10 s; TERM to its own group, 1 s, KILL): can the kill hit a group that is not the child (the child ended and was reaped before the kill)? Is the child always reaped? What stays after a KILL inside the write transaction of the pass (the savepoint of a duty)? The needs-you item of a `.lock` file that stays: can it name a lock of a git that still runs?
5. The outcome `resume_refused` (T-386, D244-F3): can a worker get attempts without limit by ending fast? Where is "1 fresh launch for each answer" recorded, and does it survive a restart? Does the attempt floor change any other rule that counts attempts (the escalation, the backoff, FM rows)?
6. The Windows detached child that fails closed (T-383, D242-F3) and the git version check in the doctor and at the start of fleetd (T-389): a refusal with no row and no job; the version parser on odd version texts.
7. If it landed: the hold of dispatch for a profile whose conformance is "not proved" (T-384, D244-F1): can the hold be skipped, and can it stay for ever with no cause shown?

Rules that each change must keep: a pass never waits for a job (A23) and runs no SQL statement for each row; a row is written before its effect (A24); fleetd never stops a process that it did not start; no OS `cfg` outside `horch-platform` (PL-2); SR-8 and FR-10.

## Output

`docs/specs/fleetd/reviews/delta-review-2-fixes-of-findings.md`: `## Scope` (base, tip, commits read); `## Findings`: each with an id (`D254-F1` ...), a class (Critical, Important, Minor), the file and line, a concrete order of events, and the smallest fix; for each finding of the first 3 reviews 1 line: fixed, fixed with a new fault (which), or not fixed; `## Checked and good`; `## Verdict`. Label each finding "verified by reading" (your sandbox cannot run the test wrapper: do not try; write the exact test command that would prove a finding). No finding without a file and line. Do not fix code.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. Never install the build or a tool.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch` (you may read a message that horch sent to you in `~/.local/state/horch/messages/w3R/`), `~/.local/bin`, `/fleet`.
3. Never run pkill or killall. Write 1 `horch ledger progress` line each 10 minutes.

## Done when

The Check passes and each commit of the scope is named in the report as read.
