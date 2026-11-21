# The whole flow test burns 2 of 3 attempts of each review task, and times out under load ("timed out waiting for T-3 done")

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test taskledger -- the_whole_flow_on_the_fake_herdr && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/taskledger.rs`

## Fault

`the_whole_flow_on_the_fake_herdr` in `crates/horch-e2e/tests/taskledger.rs` fails about 1 time in 14 with "timed out waiting for T-3 done" (seen by tasks T-336, T-384 and T-289). Measured by task T-289 in 3 kept worlds that passed: T-3 is in `verifying` for 240 s of the 480 s bound (`wait_pipeline`). The 3 review tasks of T-3 (T-7, T-8, T-9) need 3 attempts each: attempt 1 is rejected 8 s after its start; attempt 2 crashes about 40 s later; attempt 3 starts after the retry backoff (about 3 minutes) and is verified. Under load the bound runs out. Not measured, a hypothesis of T-289: the test writes the findings files only in its poll loop, so the first reviewer can end before its file exists.

## Steps

1. Run the test 1 time and keep its world. For T-7, T-8 and T-9 read each attempt: its end reason and what the verifier said (why "rejected"), and why attempt 2 "crashed" (which process ended, with what output). Write the facts in the commit message.
2. Decide by the facts. (a) The test is wrong: for example the fake reviewer ends before the test gave it its findings file. Then the test must give each reviewer what it needs before or at its start (a fixture that is there when the attempt starts, or a fake reviewer that waits for its file), so that attempt 1 is verified. (b) The product is wrong: a review attempt that did its work is rejected, or a retry attempt crashes at its start (for example it takes a tree or a path that the first attempt still holds). That is a product fault: send `QUESTION:` with the facts and the file list before you edit a product file.
3. After the fix: each review task of the flow is verified at attempt 1 (assert it: the count of attempts of T-7, T-8, T-9), and the wait for T-3 is short. Do not make the bound longer. Run the test 10 times (at most 4 at the same time) and give the time of each.
4. Say if the same pattern is in the other review tests of the file (`a_critical_finding_fails_until_a_fix_is_reviewed_again`, `a_push_review_task_ends_done_by_its_findings_file_and_its_task_lands`): how many attempts does their review take?

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `/fleet`. Never install the build.
3. No flaky test, no `#[ignore]`, no weaker assert, no longer bound, no expected-failures entry.
4. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*` (remove only your exact kept folder). Never touch a process of another fleet. 1 `horch ledger progress` line each 5 minutes while a long test runs.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish` (task T-289 lands a change of this test: start from it).
6. Before DONE: no process of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes, the 10 runs have 0 failures, and each review task of the flow is verified at attempt 1 or the commit message proves why not.

## Added 17:57Z: the cause is found and a fix exists (start here)

Local branch `fleet/fleetd-finish-T289-fixB-258-wip`, commit de3d30a6 (only `taskledger.rs`): `git cherry-pick de3d30a6` in your worktree. Cause: attempt 1 of each review is rejected because the test writes the findings files only inside `land_by_push`, after the review task ended; attempt 2 crashes with "its spawn failed: no record matches <id>" because a pass of `watch --once` with fleetd down launched the retry and the session record waited in the spool. Fix in the commit: a `FindingsWriter` thread writes each findings file when the review task exists, and the flow asserts attempts = 1 for all 9 reviews. 1 kept run: T-3 from verifying to done in 38 s (was 240 s). Left for you: the 10-run proof, and 1 finding to report in the DONE note (do not fix): a spawn from a `watch --once` pass with fleetd down fails on the spooled session record.
