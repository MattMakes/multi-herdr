# Land the work of task T-336 (CI4 FIX-32: 5 tests of taskledger.rs and the unknown-teammate reason) with a Check of its own tests

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test taskledger -- a_check_past_its_timeout_is_killed_with_its_children a_codex_workers_check_cannot_write_outside_its_worktree a_critical_finding_fails_until_a_fix_is_reviewed_again a_push_review_task_ends_done_by_its_findings_file_and_its_task_lands a_task_whose_attempts_all_fail_is_failed_and_its_replan_requeues_it a_launch_of_an_unknown_teammate_leaves_no_live_row && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-e2e/src/bin/fake-codex.rs`

## Goal

Task T-336 did the work: 2 commits on the local branch `keep/fleetd-finish-T336` (4f946807: the fake codex sandbox confines a check on Linux, and the check-timeout proof finds the processes on the host; d6551eea: `a_launch_of_an_unknown_teammate_leaves_no_live_row` matches the whole end reason of a failed spawn). Its tests are green on macOS 3 of 3 and on Linux (2 of them need git 2.41: proof by the next CI run). Its Check ran the whole file `taskledger` (about 15 minutes) and failed 3 times by tests of other tickets (ticket 234: the empty code file of AgentShell; task T-289: the whole flow; ticket 236: job processes that outlive the teardown). Land the 2 commits.

## Steps

1. In your worktree: `git merge --ff-only keep/fleetd-finish-T336`, then `git fetch` and `git rebase fleet/fleetd-finish` (resolve a conflict in favor of both changes; `crates/horch-e2e/tests/taskledger.rs` is a shared file: change no test other than the ones of the 2 commits).
2. Run the Check. The Check gives 6 test names to 1 `cargo test` filter list: confirm that it runs 6 tests and 0 are filtered by mistake (the output line "running 6 tests").
3. Run `horch ledger finish`. After the comment sweep, restore each comment that says why the fake sandbox confines a check or why a proof reads the processes of the host.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `/fleet`, or the ledger of another fleet. Never install the build.
3. No flaky test, no `#[ignore]`, no weaker assert.
4. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Never touch a process of another fleet.
5. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and the 2 commits of `keep/fleetd-finish-T336` are on the run branch.
