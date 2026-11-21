# p1_pass: the heartbeat gap of a scan of 10000 folders is over 30 s

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p1_pass --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p1_pass.rs`, `crates/horch-e2e/expected-failures/p1_pass.txt`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch/src/cmd/fleetd.rs`

## Goal

The test `scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart` is on the list `crates/horch-e2e/expected-failures/p1_pass.txt`. Task T-39 measured a gap of 38845 ms between 2 heartbeats; the limit is 30000 ms. Find the cause, fix it, and remove the entry.

## Read first

- The test in `crates/horch-e2e/tests/p1_pass.rs`.
- `docs/specs/fleetd/design.md`: the rule of the heartbeat during a long scan (search for "heartbeat" and "cursor").
- The pass loop: `crates/horch/src/cmd/fleetd.rs` (`run_loop`) and the scan steps in `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.

## Steps

1. Measure first. Run the test 3 times through `scripts/dev-sandbox.sh` and record each heartbeat gap. Record the 1-minute load of the machine (`uptime`) for each run. Find which step of the pass holds the time (add timing output to the test log, not to the product).
2. Decide by evidence: (a) the product writes the heartbeat only between passes and 1 scan step is too long: bound the work of 1 pass (the cursor exists for this) or write the heartbeat during the scan, as the design says; (b) the test fixture is slow, not the product: fix the fixture. A result that depends on machine load is not accepted: the rule is a limit of the product, so the product must hold it by design (bounded work for each pass), not by a fast machine.
3. Write the commit message with the measured numbers before and after.
4. Remove the entry from the expected-failures list. Run the test 3 times.

## Out of scope

Other tests of the file. A higher limit in the test.

## Done when

The test passes 3 times with no entry on the list, the commit message has the numbers, and the Check passes.

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



## Added 2026-11-21T02:35Z: shared files (read this first)

To let tasks run at the same time, the ledger Own list of this task does not hold each file that the ticket names. A file that the ticket text names is still in your scope. Other tasks edit other tests or functions of the same files at the same time: `crates/horch-e2e/tests/p3_refusals.rs` and its expected-failures list (tickets 164, 166, 168), `crates/horch/src/cmd/taskledger.rs` (tickets 166, 167), `crates/horch-e2e/tests/p3_unlanded.rs` (tickets 167, 168), `crates/horch-e2e/tests/taskledger.rs` and its list (tickets 168, 192), `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` (tickets 42, 73, 192). Rules: change only the tests and functions of your ticket in a shared file; do not reformat or move other code there; before `horch ledger finish` and again when the reviews are in, run `git fetch` and `git rebase fleet/fleetd-finish` and resolve a conflict in favor of both changes; then run the Check again.

The host has a high load from another fleet of the operator (not ours; never touch its processes). A test that fails only by a time limit under load: send `NOTE:` with the test and the line, and run 1 more time. Do not make a time limit larger without a measured cause. Write 1 `horch ledger progress` line each 5 minutes while a long test runs. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall.
