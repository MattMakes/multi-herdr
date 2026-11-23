# Review finding D244-F3 (Important): a resume of a session that is gone burns an attempt, and at the last attempt no fresh worker gets the answer (RR-12)

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p6_questions && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan --test push_dispatch && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/spawn.rs`, `crates/horch-e2e/tests/p6_questions.rs`

Source: `docs/specs/fleetd/reviews/delta-review-custody-and-conformance.md` on the run branch. Read the finding first. Also read the commits of task T-300 (`git log --grep 'T-300'`): it made the resume of a session with the answer (`resumed_from_session`).

## Fault

Design RR-12 (`docs/specs/fleetd/design.md:165`): after the operator answers a question, fleetd resumes the worker with the answer, or starts a fresh worker from the question, the answer and the committed work. Now (`crates/horch/src/cmd/spawn.rs:246`): the next worker gets the recorded session id with no check that the session exists. When the agent lost the session, the adapter refuses, the attempt ends `no_result`, 1 attempt of the task is burned and the retry backoff starts. At the last allowed attempt the task is escalated and no fresh worker ever gets the question and the answer. The e2e test of T-300 expects the burned attempt.

## Goal

A resume that fails because the session is gone is a fault of the mechanism, not of the task: fleetd clears the resume request (a row first) and starts 1 fresh launch with the question, the answer and the committed work, with no use of the retry budget of the task and no backoff of a failure. The fresh launch is bounded: 1 time for each answer, so a CLI that always refuses cannot loop. Each other end of a resumed attempt (the agent worked and failed) counts as today.

## Steps

1. Read how each adapter (claude, codex, the others) starts a resume and how a refusal looks: exit code, text, time. Say how fleetd tells "the session is gone" from "the agent started and then failed" without a guess on free text when the adapter gives a code; if only text exists, name the exact text of each CLI and keep it in the adapter, and treat an end of a resumed attempt within the start window with no result and no session activity as "the resume was refused". Send `QUESTION:` with the options if no safe signal exists.
2. Implement in the smallest place. If the decision lives in fleetd code outside your Own (the handlers of a worker end, the dispatch of a question), send `QUESTION:` with the file list before you edit.
3. Tests: with 1 attempt left, a stale session still gives a fresh worker with the question and the answer, and the count of attempts of the task does not grow; a second refusal for the same answer does not loop; the test of T-300 moves to the designed effect (no burned attempt), and the commit message says so. Each new test fails before the fix. Run `p6_questions` 3 times.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`, or the ledger of another fleet. Never install the build.
3. A pass never waits for a job and runs no SQL statement for each row; a row is written before its effect; no OS `cfg` outside `horch-platform`. No flaky test, no `#[ignore]`, no weaker assert, no expected-failures entry.
4. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Never touch a process of another fleet. At most 4 test processes at the same time. 1 `horch ledger progress` line each 5 minutes while a long test runs.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit and restore each comment that gives a security rule, an invariant, a bound or a design id.
6. Before DONE: no process of the unit is alive (`ps`), and the sandbox folder is removed.

## Done when

The Check passes and RR-12 holds at the last attempt.
