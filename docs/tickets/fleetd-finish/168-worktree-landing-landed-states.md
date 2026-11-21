# Worktree lifecycle: the states `landing` and `landed` of the design do not exist, so the tree of landed work is never removed at the land

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test worktree_reconcile --test worktree_reconcile_acting --test worktree_remove --test worktree_reuse && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p3_refusals --test p3_worktrees && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test taskledger -- the_whole_flow_on_the_fake_herdr && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/`, `crates/horch-core/src/fleetd/integration/application/landing.rs`, `crates/horch-core/src/fleetd/integration/application/branches.rs`, `crates/horch-core/tests/worktree_reconcile.rs`, `crates/horch-core/tests/worktree_reconcile_acting.rs`, `crates/horch-core/tests/worktree_remove.rs`, `crates/horch-core/tests/worktree_reuse.rs`, `crates/horch-e2e/tests/p3_refusals.rs`, `crates/horch-e2e/tests/p3_worktrees.rs`, `crates/horch-e2e/tests/taskledger.rs`, `crates/horch-e2e/expected-failures/taskledger.txt`

## Goal

`docs/specs/fleetd/design.md`, section of the worktrees, table "Lifecycle" (about lines 1030 to 1041):

- `active`: "The attempt ends: verified → `landing`; retried → back to `created` ...; owner dead → `orphaned`."
- `landing`: "Its branch is in the landing state machine ... Its branch is `landed` on the feature branch → `landed`. Its branch is `abandoned` → fleetd queues a rescue job; the rescue is verified → `rescued`."
- `landed`: "Nothing in it is unique any more. fleetd queues a `worktree_remove` job, and deletes the branch only if it is merged. The job passed → `removed`."

Tasks T-201 and T-89 found: the product has no such states. The tree of an attempt that ended verified goes `active` → `orphaned` ("its owner attempt ended") → `rescued`, and `reconcile.rs` removes a rescued row only when its idea is merged. So a tree of landed work stays on disk until the whole idea is merged, each verified attempt makes a rescue ref that the design does not ask for, and a resolver or fix attempt cannot run in a `landing` tree. The test `the_whole_flow_on_the_fake_herdr` (`crates/horch-e2e/tests/taskledger.rs`) waits for "the worktree of T-2 is gone" and is on `crates/horch-e2e/expected-failures/taskledger.txt` with this ticket as owner.

## Steps

1. Read the whole section of the design (lifecycle table, the reconciler, the landing state machine of a task branch, FM rows of worktrees). Read `worktree_custody/domain/worktree.rs` (the states and the legal moves), `application/reconcile.rs`, `application/remove.rs`, `integration/application/landing.rs` and `branches.rs`. Check the schema of `worktrees.state` (a CHECK constraint needs a migration; then send `QUESTION:` first, because the schema file is not in your Own).
2. Failing tests first (core): (a) the attempt ends verified → the row is `landing`, no rescue job; (b) its task branch becomes `landed` → the row is `landed`, 1 `worktree_remove` job, the branch is deleted only if it is merged, the job passed → `removed`; (c) its task branch becomes `abandoned` → a rescue job, verified → `rescued`; (d) a retried attempt → `created` and the next attempt uses the same tree; (e) owner dead → `orphaned` as today.
3. Implement the 2 states and the moves in the domain, the reconciler and the place where a task branch changes its landing state. The refusals of RR-13 apply to the removal of a `landed` tree too (dirty, untracked file, stash, unique commits): a refusal keeps the tree and opens 1 needs-you item.
4. e2e: `the_whole_flow_on_the_fake_herdr` passes; remove its entry so that `taskledger.txt` has only comment lines. In `p3_refusals.rs`, task T-89 wrote the tests of the cleanup after a land in the form "the tree is rescued, and removed when its idea is merged": change them to the form of the design (verified → `landing` → `landed` → removed at the land of the branch), and keep 1 test for the path orphaned → rescued → removed at the merge of the idea. Do not remove a `#[ignore = "flaky until T-199 ..."]` mark: that is the next task.
5. Write in the commit message each legal move of the state machine after your change, with the design line.

## Out of scope

The removal race, the double rescue, the gate tree and the scratch tree (ticket 164, the next task; it changes the same files after you).

## Done when

The tests of step 2 pass, `the_whole_flow_on_the_fake_herdr` passes with no list entry, and the Check passes.

## Added 2026-11-20: RR-15 sees the same gap

`rr15_landed_work_of_a_done_task_is_not_listed` (`crates/horch-e2e/tests/p3_unlanded.rs`) is on the temporary list `crates/horch-e2e/expected-failures/p3_unlanded.txt` with this ticket (T-207) as owner. The branch of the task lands in a bundle (`branches.landing_state` is `landed`, the task is `done`), but `horch ledger unfinished` still lists the tree: `import.rs` `fn work_landed` needs `landed_commit_of()`, which reads `worktrees.landed_commit`, and only the legacy `finish/land.rs` writes that column. A landing through a bundle never sets it. Fix it with the state `landed`: the place that moves the tree to `landed` records the landed commit, or `work_landed` reads the landing state of the branch (1 source of the fact; say which in the commit message). Then remove the entry and the `expected::check` wrap, and add `--test p3_unlanded` to the runs before finish. The orchestrator grants you these files in addition to the Own list: `crates/horch-e2e/tests/p3_unlanded.rs`, `crates/horch-e2e/expected-failures/p3_unlanded.txt`, and the file that holds `fn work_landed` (`import.rs`).

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


## Added case: FM-27

The test `fm27_only_landed_or_rescued_trees_are_removed` of
`crates/horch-e2e/tests/p3_worktrees.rs` fails each run for the same cause.
It has an entry in `crates/horch-e2e/expected-failures/p3_worktrees.txt`.
Make the test green and remove the entry. Remove the file if it is then empty.


## Added 2026-11-21T02:35Z: shared files (read this first)

To let tasks run at the same time, the ledger Own list of this task does not hold each file that the ticket names. A file that the ticket text names is still in your scope. Other tasks edit other tests or functions of the same files at the same time: `crates/horch-e2e/tests/p3_refusals.rs` and its expected-failures list (tickets 164, 166, 168), `crates/horch/src/cmd/taskledger.rs` (tickets 166, 167), `crates/horch-e2e/tests/p3_unlanded.rs` (tickets 167, 168), `crates/horch-e2e/tests/taskledger.rs` and its list (tickets 168, 192), `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` (tickets 42, 73, 192). Rules: change only the tests and functions of your ticket in a shared file; do not reformat or move other code there; before `horch ledger finish` and again when the reviews are in, run `git fetch` and `git rebase fleet/fleetd-finish` and resolve a conflict in favor of both changes; then run the Check again.

The host has a high load from another fleet of the operator (not ours; never touch its processes). A test that fails only by a time limit under load: send `NOTE:` with the test and the line, and run 1 more time. Do not make a time limit larger without a measured cause. Write 1 `horch ledger progress` line each 5 minutes while a long test runs. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall.

## Added 2026-11-21T02:35Z: the full list of marks that this task removes

When the states `landing` and `landed` work, make each of these tests green, remove its `expected::check` wrap and its list line, and run it 3 times:

- `crates/horch-e2e/expected-failures/taskledger.txt`: `the_whole_flow_on_the_fake_herdr` and `a_task_finishes_through_review_land_pr_and_cleanup` (do NOT touch the 2 lines owned by ticket 192).
- `crates/horch-e2e/expected-failures/p3_unlanded.txt`: `rr15_landed_work_of_a_done_task_is_not_listed`.
- `crates/horch-e2e/expected-failures/p3_worktrees.txt`: `fm27_only_landed_or_rescued_trees_are_removed`.

The file `crates/horch-e2e/tests/p3_worktrees.rs` takes 10 to 13 minutes (its 8 tests run 1 at a time). The check limit of this task is 5400 s.
