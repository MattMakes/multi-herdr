# RR-13: a drop of a worktree whose HEAD moved during the checks is accepted (the refusal is lost)

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p3_refusals && scripts/dev-sandbox.sh cargo test -p horch --bin horch --locked cmd::taskledger && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/taskledger.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs`, `crates/horch-e2e/tests/p3_refusals.rs`, `crates/horch-e2e/expected-failures/p3_refusals.txt`

## Goal

`docs/specs/fleetd/design.md` line 166, RR-13: "`horch worktree drop` and each cleanup shall refuse a dirty tree, a branch that a `git stash` entry names, unpublished unique commits and a HEAD that moved during the checks. Phase 3 test: a drop for each of the 4 conditions is refused, and the tree stays." Lines 1420 (W6) and 1660 say the same. Task T-89 found: the product lost the fourth refusal. `fn drop_facts` (`crates/horch/src/cmd/taskledger.rs`) reads HEAD, then `rev-list`, and never reads HEAD again; `fn refuse_drop` (`crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs`) has no moved-HEAD check; the drop is accepted.

The strict test `rr13_drop_with_a_head_that_moved_during_the_checks_is_refused_and_the_tree_stays` (`crates/horch-e2e/tests/p3_refusals.rs`) is on the temporary list `crates/horch-e2e/expected-failures/p3_refusals.txt` with this ticket as owner. A second test of T-89 proves that no commit is lost today (the rescue ref holds the commit that came during the checks); it stays.

## Steps

1. Read how the facts of a drop go from the CLI to fleetd (the request, `drop_facts`, `refuse_drop`) and where the old code read HEAD the second time (`git log -S` for the old check on the base `fleet/fleetd-build`).
2. Fix by the design: the HEAD that the checks used and the HEAD at the decision are compared at the place that decides (fleetd reads the HEAD itself in the job that acts, or the request carries the checked HEAD and the job refuses when the tree has another HEAD). The check and the act must not have a window between them that a worker commit can enter: say in the commit message why your form has none.
3. The same for "each cleanup" of RR-13: list each cleanup path (the remove job after a merge, `dismiss --cleanup`, triage apply) and say for each one where the moved HEAD is refused; add a core test for each one that has none.
4. Remove the entry of the list (delete the file if it has no active entry) and the `expected::check` wrap. Run `p3_refusals` 3 times.

## Out of scope

The removal race and the gate tree (ticket 164).

## Done when

The strict test passes with no list entry, and the Check passes.

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



## Added 2026-11-21T02:35Z: shared files (read this first)

To let tasks run at the same time, the ledger Own list of this task does not hold each file that the ticket names. A file that the ticket text names is still in your scope. Other tasks edit other tests or functions of the same files at the same time: `crates/horch-e2e/tests/p3_refusals.rs` and its expected-failures list (tickets 164, 166, 168), `crates/horch/src/cmd/taskledger.rs` (tickets 166, 167), `crates/horch-e2e/tests/p3_unlanded.rs` (tickets 167, 168), `crates/horch-e2e/tests/taskledger.rs` and its list (tickets 168, 192), `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` (tickets 42, 73, 192). Rules: change only the tests and functions of your ticket in a shared file; do not reformat or move other code there; before `horch ledger finish` and again when the reviews are in, run `git fetch` and `git rebase fleet/fleetd-finish` and resolve a conflict in favor of both changes; then run the Check again.

The host has a high load from another fleet of the operator (not ours; never touch its processes). A test that fails only by a time limit under load: send `NOTE:` with the test and the line, and run 1 more time. Do not make a time limit larger without a measured cause. Write 1 `horch ledger progress` line each 5 minutes while a long test runs. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall.
