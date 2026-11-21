# 73: Fix R4-F4: read base branch tips through the sandboxed git gateway before opening the pass write transaction (SR-4, RR-5)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test push_dispatch --test fleetd_pass_p4a --test fleetd_handlers_worker && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/tests/push_dispatch.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p5.md` (finding R4-F4 and section "Tests that do not prove their rule" row 4) and `docs/specs/fleetd/design.md` (rules SR-4 and RR-5).

## Goal

Task binding (`dispatch::bind` and `dispatch::dispatch_due`) never spawns an external `git` process while holding the ledger write transaction and never uses the unsandboxed `taskledger::git::Git::rev` inside the fleet pass. Instead, base branch tips are read before the write transaction opens through the sandboxed fleetd git gateway (`IntegrationGitGateway` / `CliGitGateway`), and `dispatch::bind` consumes those pre-read immutable tips inside the transaction after validating that the target task row is still `ready` and eligible.

## Requirements

- **R4-F4 (Important — SR-4, RR-5)**:
  - **Finding text and file:line**:
    `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:315`; `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:1404`. Binding a task calls the plain `taskledger::git::Git::rev` while the fleet pass holds its ledger transaction. This is an external project operation inside the single-writer transaction, and it does not use the sandboxed fleetd git gateway. A slow or stuck `git rev-parse` blocks every ledger write and can hang the whole fleet pass. The smallest fix is to read the base tip before opening the write transaction through the sandboxed fleetd git gateway, then pass the immutable tip into the row decision and validate that the target row is still eligible.
  - **Design rules**:
    - **SR-4** (`docs/specs/fleetd/design.md:180`) & fleetd git rule (`docs/specs/fleetd/design.md`, Q26): fleetd runs project git operations in its sandbox with hooks, fsmonitor, signing, editors, and project program keys disabled (`CliWorktreeGit::checked` / `CliGitGateway`).
    - **RR-5** (`docs/specs/fleetd/design.md:158`): "From Phase 5, exactly one process shall write the ledger. Every other component writes files that it ingests. Exactly one fleetd shall run for one ledger: it holds an exclusive file lock for its whole life, and each write transaction checks its generation." No external process runs inside the single-writer SQLite transaction.

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

## Steps

1. Write the failing test first in `crates/horch-core/tests/push_dispatch.rs`:
   - Add test function `bind_uses_pre_read_sandboxed_base_tips_and_runs_no_git_in_write_tx`:
     - Set up a `World` with a ready task that needs a new worktree, and configure `World.git` with a bogus binary path (for example `Git::new("/nonexistent/git-must-not-run-inside-tx")`).
     - Pre-read the base tips outside the write transaction using `dispatch::read_base_tips` with a `CliGitGateway` (from `effects::integration_git(&w.fleet_root, &[w.root.clone()])`), asserting that a repository `.git/config` with `core.fsmonitor = "evil"` is refused by the sandboxed gateway before the transaction opens.
     - With a clean repo config, pre-read the base tips through the sandboxed `IntegrationGitGateway`, pass them into `DispatchCtx` (with the non-existent `Git` binary or without `Git` on `DispatchCtx`), and run `w.dispatch(...)` / `bind(...)` inside `store.immediate(|tx| ...)`.
     - Assert that `bind` succeeds, records the exact `base_commit` on the new worktree row, and spawns no `git` subprocess while `tx` is open.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test push_dispatch -- bind_uses_pre_read_sandboxed_base_tips_and_runs_no_git_in_write_tx` and see it fail because `new_tree` at `dispatch.rs:315` calls `git.rev(root, &base)` inside the transaction.
2. In `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs` and `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`:
   - In `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`:
     - Add a function `pub fn read_base_tips(conn: &Connection, gw: &dyn IntegrationGitGateway) -> Result<BTreeMap<(String, String), String>>` (keyed by `(project_root, branch_name)` -> `commit_sha`) that reads the candidate base branch tips (`tree_base(conn, &run, &task)`) for ready tasks that have no existing worktree row, using `gw.tip(Path::new(&run.project_root), &format!("refs/heads/{base}"))` before the write transaction opens.
     - Carry the pre-read base tips on `DispatchCtx` (for example `pub base_tips: Option<&'a BTreeMap<(String, String), String>>`, or replace `ctx.git.rev(root, &base)` when `base_tips` is provided so `new_tree` looks up `(run.project_root, base)` in `base_tips` without spawning `git` inside `tx`).
     - In `bind` (lines 141-235): validate that the target task is still `TaskStatus::Ready`, not cancel-requested, and has no open attempt, and look up the immutable base tip from `ctx`. If a required base tip is missing from the pre-read map, fail that task's `dispatch_bind` savepoint cleanly without running external git inside the fleet pass transaction.
   - In `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`:
     - In `read_facts` (Part 1, before `decide` opens `TransactionBehavior::Immediate`, around line 1075), read the dispatch base tips using `CliGitGateway::new(effects::integration_git(&self.settings.fleet_root, &repos))` and store them on `Read`.
     - In `dispatch_rows` (line 1404), pass the pre-read `base_tips` on `DispatchCtx` into `dispatch::dispatch_due` so no `git` process ever runs during `decide`.
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Changing worktree creation job execution in `crates/horch-core/src/fleetd/worktree_custody/adapters/executors.rs`.

## Done when

`bind_uses_pre_read_sandboxed_base_tips_and_runs_no_git_in_write_tx` passes, `Pass::decide` runs no external `git` process while holding the write transaction, base tips are read in Part 1 through `CliGitGateway`, and the ticket Check passes.


## Added 2026-11-21T02:35Z: shared files (read this first)

To let tasks run at the same time, the ledger Own list of this task does not hold each file that the ticket names. A file that the ticket text names is still in your scope. Other tasks edit other tests or functions of the same files at the same time: `crates/horch-e2e/tests/p3_refusals.rs` and its expected-failures list (tickets 164, 166, 168), `crates/horch/src/cmd/taskledger.rs` (tickets 166, 167), `crates/horch-e2e/tests/p3_unlanded.rs` (tickets 167, 168), `crates/horch-e2e/tests/taskledger.rs` and its list (tickets 168, 192), `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` (tickets 42, 73, 192). Rules: change only the tests and functions of your ticket in a shared file; do not reformat or move other code there; before `horch ledger finish` and again when the reviews are in, run `git fetch` and `git rebase fleet/fleetd-finish` and resolve a conflict in favor of both changes; then run the Check again.

The host has a high load from another fleet of the operator (not ours; never touch its processes). A test that fails only by a time limit under load: send `NOTE:` with the test and the line, and run 1 more time. Do not make a time limit larger without a measured cause. Write 1 `horch ledger progress` line each 5 minutes while a long test runs. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall.
