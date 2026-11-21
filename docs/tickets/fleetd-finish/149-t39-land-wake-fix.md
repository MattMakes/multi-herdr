# Land the fix of task T-39 (R1-F3, the pass loop wakes on events) with a longer check time

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --bin horch --locked cmd::fleetd && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p1_pass --locked -- --skip scans_of_10000 && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/fleetd.rs`, `crates/horch-e2e/tests/p1_pass.rs`, `crates/horch-e2e/expected-failures/p1_pass.txt`

## Goal

Task T-39 (ticket `37-fix-r1-f3-pass-loop-event-wake.md`) fixed finding R1-F3: fleetd wakes its pass loop on a request file, an attempt outbox event, a job result file or a ledger commit. Its Check needed more than the limit of 1200 s, because the test `scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart` (an expected failure, ticket 42 repairs it) runs 9 to 19 min before it fails. This task lands the same 4 commits. Its Check skips that 1 test and has a limit of 3600 s.

## Steps

1. In your worktree: `git cherry-pick fleet/fleetd-finish..keep/fleetd-finish-T39` (the local branch `keep/fleetd-finish-T39`, tip cffe89354bce41dd122d3a94c7a1d19730af40ad, holds the 4 commits of T-39). Keep each commit message.
2. Do not change the code or the tests. If the cherry-pick has a conflict, resolve it with no change of an assert, and name the conflict in the commit message.
3. Run the Check 1 time. Then run `horch ledger finish`.

## Out of scope

The 4 tests on `crates/horch-e2e/expected-failures/p1_pass.txt` (tickets 41 and 42).

## Done when

The 4 commits of T-39 are on the run branch and the Check passes.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

