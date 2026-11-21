# Land the fix of task T-103 (4 unit tests of the horch binary) with a valid Check command

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --bin horch --locked -- cmd::context::tests::ctx_06_windows_view_lists_claude_and_codex_teammates cmd::messaging::tests::trf_45_note_and_ledger_progress_are_1_command_and_write_no_json cmd::taskledger::tests::no_allowed_ledger_subcommand_takes_a_check_command tests::help_lists_the_same_commands_in_the_same_order && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/context.rs`, `crates/horch/src/cmd/messaging.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch/src/main.rs`

## Goal

Task T-103 (ticket `129-ci-fix10-horch-bin-unit-unexplained.md`) fixed 4 unit tests. Its Check gave cargo 4 test names before `--`, which cargo refuses, and it had a 3-run loop. The worker ran the valid form 3 times with 4 of 4 green. The local branch `keep/fleetd-finish-T103` (tip 57f7a5ce) holds the commits. This task lands them.

## Steps

1. In your worktree: `git cherry-pick fleet/fleetd-finish..keep/fleetd-finish-T103`. Keep each commit message. Resolve a conflict with no change of an assert.
2. Do not change the tests. Run the Check 1 time. Then run `horch ledger finish`.

## Out of scope

New test changes.

## Done when

The commits of T-103 are on the run branch and the Check passes.

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

