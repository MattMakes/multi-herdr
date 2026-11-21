# R8-F4 and R8-F5 (Minor): 2 comment sweeps removed the reason of code that needs it

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked execution::store && cargo fmt --all --check`
**Own:** `crates/horch-core/src/execution/store.rs`, `crates/horch/tests/support/mod.rs`

Source: the end review `docs/specs/fleetd/reviews/end-review-a.md` (task T-297, reviewed tip 08c6d4ff). Read its finding first; it has the file and line of each fact.

## Goal

1. R8-F4: commit f1e2b0b3 removed the comment on `has_session_records_table` (`crates/horch-core/src/execution/store.rs` near line 732). It said why a reader can see a new ledger before the migration commits the table `session_records`. Restore that reason in 2 lines (`git show f1e2b0b3` has the old text).
2. R8-F5: commit 54ebe0e8 removed the reason above `#[allow(dead_code)]` at `crates/horch/tests/support/mod.rs:4`. Prefer the better fix: make the allowance narrow (only on the helper or module that needs it), and keep 1 line with the reason. If narrow is not possible, restore the reason line.
3. The finish of this task runs a comment sweep: after it, check that both reasons are still there; if the sweep removed them, restore them and tell the orchestrator.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build or a tool. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`. Each `claude` command of a test or of the product removes it from the environment.
5. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry without an answer of the orchestrator.
6. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never run `cargo test -p horch --bin horch` with a filter that matches `watch_once` before task T-287 has landed (it starts a process chain).
7. The host has a high load from another fleet of the operator: never touch its processes. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
8. Other tasks edit the same large files (`pass.rs`, `handlers_worker.rs`, `cmd/fleetd.rs`): change only the functions of your ticket there; `git fetch` and `git rebase fleet/fleetd-finish` before `horch ledger finish` and again when the reviews are in; resolve a conflict in favor of both changes.
9. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

Both reasons are in the code on the run branch and the Check passes.
