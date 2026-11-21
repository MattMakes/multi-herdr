# Land the fix of task T-106 (p7_docs_sync and p7_release) with a longer check time

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p7_docs_sync --test p7_release --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p7_docs_sync.rs`, `crates/horch-e2e/tests/p7_release.rs`

## Goal

Task T-106 (ticket `132-ci-fix10-e2e-p7-docs-sync-release.md`) fixed the red tests of `p7_docs_sync.rs` and `p7_release.rs`. Its Check ran the 2 files 3 times and took about 25 min; the limit of the finish step was 1200 s, so the step timed out with no red test. The worker proved 3 green runs of 3 before the finish. This task lands the same commits with 1 run in the Check and a limit of 3600 s.

## Steps

1. In your worktree: `git cherry-pick fleet/fleetd-finish..keep/fleetd-finish-T106` (the local branch `keep/fleetd-finish-T106`, tip 5d349c8fd665a9b9e2b7905304bcebec0df02e8b, holds the commits of T-106). Keep each commit message.
2. Do not change the tests. If the cherry-pick has a conflict, resolve it with no change of an assert.
3. Run the Check 1 time. Then run `horch ledger finish`.

## Out of scope

New test changes.

## Done when

The commits of T-106 are on the run branch and the Check passes.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

