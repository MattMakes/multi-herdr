# RR-14: `horch ledger done --unlanded` in a fleetd attempt does not park the task

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p3_unlanded --test p4_verifier && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked fleetd::task_execution && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/src/fleetd/task_execution/application/ingest.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch-e2e/tests/p3_unlanded.rs`, `crates/horch-e2e/tests/p4_verifier.rs`

## Goal

`docs/specs/fleetd/design.md` line 167, RR-14: "A task that cannot land shall park with its worktree, its dependents shall stay blocked, and a requeue shall resume it on the same worktree (04 L5)." Task T-89 found: under fleetd, `horch ledger done --unlanded` (`crates/horch/src/cmd/taskledger.rs` `write_completed`) writes `result.json` with `notes_for_next` "unlanded: <reason>", and nothing in fleetd reads it. Evidence: a bound task with 1 commit, `done --unlanded`: the attempt ends `verified`, the task is `done`, the tree is rescued, and the task is not parked. `held_reason = unlanded` is written only by `finish::complete` / `mark_done`, which no fleetd code calls; `ledger.rs:136` says "Phase 4 moves the unlanded park to the verifier". So work that did not land is recorded as done, and its dependents start.

The file `p3_unlanded.rs` tests the rest of RR-14 and RR-15 today with a fixture that parks the task directly (`finish::complete` while fleetd is down).

## Steps

1. Read the design: RR-14, RR-15, the result file of an attempt, the verifier, and what "cannot land" means under fleetd (who says it: the worker in its result, the verifier, the landing bundle). If the design does not say how a worker reports "unlanded" in `result.json`, send `QUESTION:` with the lines before you choose a form.
2. Failing e2e test first, in `p3_unlanded.rs`: a worker in a fleetd attempt runs `horch ledger done --unlanded "<reason>"`; after the passes the task is parked (`held_reason` unlanded with the reason), its tree is kept (not rescued, not removed), its dependent stays `pending`, and a requeue resumes in the same tree.
3. Fix: the result carries the unlanded state in a typed field (not in the free text `notes_for_next`, which is worker text), and the verifier parks the task. The reason is untrusted text of a worker (SR-6): escape it where it is shown.
4. Change the fixture of `p3_unlanded.rs` to the real command where that is possible.

## Out of scope

The removal race (ticket 164). RR-13 (ticket 166).

## Done when

The new test passes and the Check passes.

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
