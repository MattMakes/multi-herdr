# Leftover on disk: fleetd never removes a released gate tree

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test gate_jobs && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p3_refusals && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/gate_tree.rs`, `crates/horch-core/tests/gate_jobs.rs`, `crates/horch-e2e/tests/p3_refusals.rs`, `crates/horch-e2e/expected-failures/p3_refusals.txt`, `crates/horch-core/src/fleetd/worktree_custody/application/remove.rs`, `crates/horch-core/src/fleetd/worktree_custody/domain/removal.rs`

## Goal

Task T-89 found this. A gate tree (`.worktrees/gate-F-<n>`, made by `gate_tree::reserve` and a `worktree_add` job) is released by `gate_tree::release` and goes to state `orphaned`, then `rescued` (a rescue job passes). After that no `worktree_remove` job is ever queued: in `reconcile.rs` `fn act` a rescued row is removed only when `idea_merged` is true, and a gate tree has no task and so no idea. The module doc says "A rescued tree with no task is never removed". So each full gate of a feature leaves 1 folder with a checkout on disk for all time. The operator rule of this product: nothing stays on disk.

The e2e test `run_gate_folder_is_added_and_removed_through_the_jobs` (`crates/horch-e2e/tests/p3_refusals.rs`) proves the fault; it is on the temporary list `crates/horch-e2e/expected-failures/p3_refusals.txt` with this ticket as owner.

## Steps

1. Read `docs/specs/fleetd/design.md`: the gate of a feature, the gate tree, the worktree states and the table "a rescued tree is removed when its idea is merged or cancelled". Find what the design says about the end of a gate tree (the feature of the gate tree is its owner).
2. If the design gives a rule (for example: the gate tree is removed when its gate job ended and its rescue ref exists, or when its feature is merged or abandoned): implement it, with the rescue ref kept. If the design gives NO rule: stop and send `QUESTION:` to the orchestrator with the exact design lines and 2 options; the orchestrator asks the operator (a change of the design text is not yours to make).
3. Tests first: a core test in `gate_jobs.rs` (reserve, release, passes, the row is `removed`, the folder is gone, the rescue ref exists), then remove the entry of `p3_refusals.txt` (delete the file if empty) and the `expected::check` wrap.
4. A tree with no task that is NOT a gate tree stays as it is now (never removed without an operator decision): add 1 test that proves it.

## Out of scope

`horch worktree triage`.

## Done when

The e2e test passes with no list entry, and the Check passes.

## Added 2026-11-20: second fault, same subject: a failed removal is never tried again

Task T-89 found this with the test `cleanup_after_land_removes_a_landed_tree_through_the_remove_job` (`crates/horch-e2e/tests/p3_refusals.rs`), which is under `#[ignore = "flaky until T-196"]`. In 1 of 4 runs the first `worktree_remove` job of a rescued tree fails: "the removal of <id> is refused, and the tree stays: the processes <pid> have their working folder in the tree" (`removal.rs` `fn guard`, `Saved::Rescued`, `cwd_pids`). `lsof -d cwd` showed only children of fleetd itself with their working folder in the tree: `git` and `sandbox-exec` of a scan or a job. So a git run of fleetd in the tree races the guard. A failed removal is never queued again (`remove.rs` `fn queue_auto_remove`, `removal_failed`), so the tree stays rescued for all time.

Steps for this part:
1. Failing test first (core test, no real race): a rescued tree whose idea is merged; the first removal is refused because a process has its working folder in the tree; the process ends; after the next passes the tree is removed. Use an injected process list or a held child that the test ends, not a timing race.
2. Fix both: (a) fleetd does not start the removal while 1 of its own jobs runs in the tree (the job queue knows this), or the guard does not count the processes of fleetd jobs that are ended before the removal; (b) a removal that was refused for a cause that can go away (a process in the tree) is tried again with the backoff of the design, and after the limit of the design it opens 1 needs-you item. A refusal for a cause that cannot go away (dirty tree, untracked file, unique commits) stays refused.
3. Remove the `#[ignore]` of the e2e test and run `p3_refusals` 10 times.
4. Report in the commit message: the legacy columns `worktrees.status` (stays open) and `landed_commit` (stays empty) after a custody removal. Find each reader of the 2 columns; if a reader shows a wrong state to the operator, fix it or send `QUESTION:`.

## Added 2026-11-20 (2): more evidence of the removal race

A second test of `p3_refusals.rs` is under `#[ignore = "flaky until T-199 ..."]`: `dismiss_cleanup_rescues_then_removes_only_the_recorded_path` (cancel, the passes rescue the tree, `dismiss --cleanup` queues the removal, the passes run it). Its `worktree_remove` job fails 4 of 5 runs with "the processes <pid> have their working folder in the tree". 1 run showed by `lsof` a `horch` process of fleetd (`horch job run`) with its working folder in the tree, and that run had 2 rescue jobs that passed for 1 tree. So: (a) job processes of fleetd (`horch job run`, `git`, `sandbox-exec`) race the guard; (b) find why 1 tree gets 2 rescue jobs, and fix it if it is a fault. Remove both `#[ignore]` marks, and run `p3_refusals` 10 times.

## Added 2026-11-20 (3): the double rescue is confirmed, and a third test waits for this ticket

`drop_of_a_clean_tree_writes_a_verified_rescue_ref_before_the_removal` (`p3_refusals.rs`) is the third test under `#[ignore = "flaky until T-199 ..."]`. Its removal job fails or times out 2 of 3 runs (the race). Second fault, confirmed: 1 drop of a worker makes 2 rescue runs for 1 tree: the refs `refs/horch/rescue/<id>/1` and `/2`, both jobs passed, same HEAD. The test asserts 1 ref and 1 rescue job. Fix: a tree with a verified rescue ref of its HEAD gets no second rescue job (find the rule that queues it: the reconcile pass, or a second request at the drop); failing core test first. Then remove the 3 `#[ignore]` marks (`git grep -n 'flaky until T-199' crates/horch-e2e`) and run `p3_refusals` 10 times.

## Added 2026-11-20 (4): a dropped scratch tree is the same case as the gate tree

A fourth test waits: `drop_of_a_merged_branch_deletes_it_and_keep_branch_keeps_it` (`p3_refusals.rs`, `#[ignore]` with owner T-199; it keeps the asserts of the removal: the removal job, `branch_deleted`, `keep_branch`). A scratch tree (made by the request `worktree scratch`) that a worker drops is rescued, and then never removed: it has no task, so `idea_merged` is false (`reconcile.rs` `fn act`). This changes step 4 of this ticket: first find in the design what ends a tree with no task that its owner dropped (a scratch tree after `drop`, a gate tree after `release`). A drop or a release is a decision of the owner of the tree, so the removal after a verified rescue ref is the likely rule; if the design does not say it, send `QUESTION:` with the lines. A tree with no task that nobody dropped or released stays (never removed without a decision): keep 1 test for that.

## Added 2026-11-20 (5): this task comes after ticket 168

Ticket 168 (task T-207) adds the states `landing` and `landed` of the design to the worktree lifecycle and changes the same files first. Read its commits before you start. The tests under `#[ignore = "flaky until T-199 ..."]` name the old task number of this ticket; they are yours.

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

## Added 2026-11-21T03:05Z: 1 more mark that this task removes

`the_whole_flow_on_the_fake_herdr` of `crates/horch-e2e/tests/taskledger.rs` is on `crates/horch-e2e/expected-failures/taskledger.txt` with this ticket as owner: its last worktree assert (near line 1432) fails only on the gate tree `gate-F-0001` (scratch, status open). When the gate tree is removed, make that test green, remove its list line and its `expected::check`, and run it 3 times: `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test taskledger -- the_whole_flow_on_the_fake_herdr`.

## Added 2026-11-21T03:36Z: 1 more ignored test that this task makes green

Task T-286 added the test `orphaned_tree_is_rescued_and_removed_when_its_idea_is_merged` to `crates/horch-e2e/tests/p3_refusals.rs` with the mark `#[ignore = "flaky until T-199"]`. It fails 2 of 2: the removal is refused with "the processes N have their working folder in the tree" (the race of T-199). When you remove the 4 older ignore marks, remove this one too: 5 marks in all, and no `#[ignore]` stays in the file. Run each of the 5 tests 3 times.
