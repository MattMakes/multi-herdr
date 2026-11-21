# `horch ledger status --json` failed 1 time with "attempt to write a readonly database": a read command runs a write statement

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --locked --test status_read_only && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/taskledger.rs`, `crates/horch-core/src/taskledger/store.rs`, `crates/horch-core/src/taskledger/mod.rs`, `crates/horch/tests/status_read_only.rs`

The status command is in `crates/horch/src/cmd/taskledger.rs`; the read-only open is in `crates/horch-core/src/taskledger/store.rs`. The test file `crates/horch/tests/status_read_only.rs` is new: you make it. If the cause is in another file, send `QUESTION:` with the file.

## Goal

Task T-201 saw this 1 time in the e2e test `the_whole_flow_on_the_fake_herdr` while fleetd ran: `horch ledger status --json` failed with "attempt to write a readonly database", SQLite "Error code 8" (the plain `SQLITE_READONLY`; WAL recovery gives 264 or 1288, so a real write statement ran on the read-only connection). The worker found no write statement in `status.rs`, `resolve_run`, `no_runs_to_show` or the callees (holds, integration, custody, orchestrator queries), and `adopt_workspace` only prints a warning. Rule of the design: a reader command (class read) opens the ledger read-only and never writes; only fleetd writes.

## Steps

1. Do not try to repeat the race by load. Make it deterministic: add a debug trace of each SQL statement of the status command (rusqlite `trace` or `profile`, behind a test hook), run status on a read-only connection against ledgers in each state that a live fleetd can leave (a WAL with frames that are not checkpointed, a schema version 1 lower, a run with each kind of row), and find the statement that is not a SELECT (also: PRAGMA that writes, CREATE TEMP, a lazy migration, `INSERT OR IGNORE` of a default row).
2. Write the failing test: the status command on a ledger in that state, with a connection that is read-only, passes and writes nothing (compare the file bytes or `PRAGMA data_version`).
3. Fix: remove the write from the read path. If the write is needed, it is a request to fleetd, not a write of the reader.
4. Add 1 test that runs each reader command of the CLI (the list of class read in the request classes) on a read-only connection.

## Done when

The cause is named with file and line in the commit message, the tests pass and the Check passes.

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

