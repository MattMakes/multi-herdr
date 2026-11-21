# p1_pass: move rr4, rr7 and fm11 to the push flow and remove their expected failures

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p1_pass --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p1_pass.rs`, `crates/horch-e2e/expected-failures/p1_pass.txt`

## Goal

3 tests of `crates/horch-e2e/tests/p1_pass.rs` are on the list `crates/horch-e2e/expected-failures/p1_pass.txt` since task T-39. Make each one prove its rule on the current product, and remove its entry.

## Read first

- `crates/horch-e2e/tests/p1_pass.rs` and `crates/horch-e2e/expected-failures/p1_pass.txt` in your worktree.
- `crates/horch-e2e/src/ledger_seed.rs` and 1 test of the push flow, for example `crates/horch-e2e/tests/p4_verifier.rs`: how a test seeds a run and how a fake worker ends a pushed task with `result.json`.
- `docs/specs/fleetd/design.md`: the rules RR-4, RR-7, FM-11 and FM-16.
- `git show feee43be`: the commit that removed the respawn of the orchestrator (FM-16).

## Tests

1. `rr4_tasks_complete_with_all_file_watch_events_dropped` and `rr7_fleetd_killed_mid_run_ends_as_a_clean_run`: the helper `complete()` (worker flow of `run_plan`) runs `horch ledger worktree new`. The product refuses it: "fleetd binds each task and makes its worktree; this command is not for a worker". Rewrite the worker flow of `run_plan` to the flow of phases 4 to 6: fleetd binds the worktree, the fake worker reads the attempt files and writes `result.json`. Keep each assert on the rule (RR-4: all tasks complete with each file-watch event dropped; RR-7: a kill of fleetd in the middle of the run ends as a clean run).
2. `fm11_fleetd_aborted_after_the_commit_does_each_effect_once_on_restart`: it asserts 1 respawn of the orchestrator. FM-16 removed that effect on purpose. Replace the respawn with a post-commit effect that the product has now (read `pass.rs` for the list of post-commit effects; do not edit it). The test must still prove: fleetd is aborted after the commit and before the effect; after the restart the effect happens exactly 1 time. Do not weaken this assert. Name the removed effect and FM-16 in the commit message.
3. Remove the 3 entries from `crates/horch-e2e/expected-failures/p1_pass.txt`. Run the test file 3 times.

## Out of scope

- `scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart` (ticket 42). Its entry stays.
- Product code. If a test shows a product fault, stop and send `BLOCKED:` with the evidence.

## Done when

The 3 tests pass 3 times with no entry on the expected-failures list, and the Check passes.

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

