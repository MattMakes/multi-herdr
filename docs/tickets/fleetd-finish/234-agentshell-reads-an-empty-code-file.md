# Flaky under load: AgentShell reads an empty exit-code file (taskledger.rs:1660, ParseIntError Empty)

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test taskledger a_land_ && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/taskledger.rs`

## Fault

The second finish Check of task T-336 failed 1 time, at a 1-minute load of 43, on `a_land_checks_outside_the_lock_and_lands_after_a_faster_task` in `crates/horch-e2e/tests/taskledger.rs`: the helper `AgentShell` read its exit-code file and got an empty text (`taskledger.rs:1660`, `ParseIntError { kind: Empty }`). Run alone the test passes 3 of 3. Probable cause (prove it): the shell writes the code with a redirect, which makes the file empty first and writes the text after; the reader waits only for the file to exist. No flaky test is accepted in this run.

## Goal

The reader never sees a part of the code. The writer makes the file whole in 1 step (write to a temp name in the same folder, then `mv`), or the reader waits for a complete line that ends with a newline, within the time limit that the helper has now. No sleep, no retry of the test, no larger time limit.

## Steps

1. Read `AgentShell` and each place that reads a file that a shell of the test writes (the same pattern can be in other helpers of this file and of `crates/horch-e2e/src`: list them; fix those in this file; name the others in the DONE note).
2. Reproduce first: a loop of the test under load made by the test itself is not allowed (other fleets use this host). Reproduce with a scratch edit that puts a pause between the truncate and the write in the shell line; see the failure; revert the scratch edit.
3. Fix. With the scratch pause back in place, the test passes; revert the pause. Run the `a_land_` tests 10 times (at most 4 test processes at the same time).
4. Task T-289 edits the test `the_whole_flow_on_the_fake_herdr` in the same file: do not touch that test; `git fetch` and rebase before the finish.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. Run each `cargo test` through `scripts/dev-sandbox.sh`. The worktree keeps its own `target/`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `/fleet`, or the ledger of another fleet. Never install the build.
3. No flaky test, no `#[ignore]`, no weaker assert, no expected-failures entry.
4. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Never touch a process of another fleet. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`.
6. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes, the scratch pause proof is in the commit message, and the 10 runs have 0 failures.
