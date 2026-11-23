# The record of the last statement of a read-only ledger is 1 process-wide slot: a flaky test and a wrong error text

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked taskledger::store && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan && scripts/dev-sandbox.sh cargo test -p horch --locked --test status_read_only && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/store.rs`

## Fault

`crates/horch-core/src/taskledger/store.rs` keeps `static LAST_READ_ONLY: Mutex<Option<(PathBuf, String)>>`. `record_statements` sets the path at each open of a read-only store and the trace callback writes the SQL of each statement of each traced connection into the same slot. So a second read-only store in the same process (a thread of fleetd, a parallel test) replaces the path and the statement. Proof: the land step of task T-355 failed 1 time on `taskledger::store::tests::a_read_only_failure_names_the_command_the_statement_and_the_ledger` (store.rs:565): the text named the ledger path of another test. In the product the same fault makes `read_only_failure` name the wrong ledger or the wrong statement when 2 read-only stores are open.

## Goal

The record belongs to its connection. `read_only_failure` names the statement and the ledger of the store that failed, with any number of read-only stores open in the process, on any thread. The test cannot fail by the work of another test. No lock that makes tests run 1 at a time, no thread-local that gives a wrong answer when a store moves to another thread, no retry.

## Steps

1. Read `record_statements`, `read_only_failure`, each caller, and the tests of the module. Task T-327 changed the trace hook in this file (the savepoint `one_snapshot` counts as a read): `git fetch` and rebase first; keep that rule and its test.
2. Design: the callback gets a context pointer to a record that the `Store` owns (for example a `Box` or `Arc<Mutex<..>>` whose life is the life of the connection; say in a SAFETY comment why the pointer is valid for each call and when the trace is removed), or the record is keyed by the connection handle. `read_only_failure` takes the store (or its record). Choose the smaller change; say why in the commit message.
3. Test first: a test that opens 2 read-only stores on 2 ledger files, runs a statement on each (the second after the first), makes the first fail, and asserts that the text names the first ledger and the first statement. It fails before the fix. Then run the lib tests of the module 20 times in parallel with the other lib tests (`scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked taskledger::` 20 times; at most 4 at the same time).
4. `horch ledger status` stays read only (task T-236): the tests `status_read_only`, `p4_status_cli`, `p7_status_cli` stay green.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `/fleet`. Never install the build.
3. No flaky test, no `#[ignore]`, no weaker assert, no serial lock for tests.
4. Never run pkill or killall. Never touch a process of another fleet. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit and restore each SAFETY comment and each comment that gives an invariant.
6. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes, the new test fails before the fix and passes after it, and the 20 runs have 0 failures.
