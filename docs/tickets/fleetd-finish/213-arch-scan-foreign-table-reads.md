# arch_scan is red: 14 SQL lines of 3 landed tasks read a table of another context

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan --test nfr --test architecture_doc && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test push_dispatch --test fleetd_pass --test worktree_reconcile && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked fleetd && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`, `crates/horch-core/src/fleetd/task_execution/application/launch.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/disk.rs`, `crates/horch-core/src/fleetd/ideas/application/sub_orchestrators.rs`

Source: the pre-gate of Phase 4 (task T-323, worker opus-69, 2026-11-21T05:41Z, tip 895e3848).

## Fault

`fitness_no_context_touches_foreign_tables` (`crates/horch-core/tests/arch_scan.rs:1880`) fails 3 of 3. 14 SQL lines name a table that another bounded context owns. They came with the set-based readers of 3 tasks of today (their Checks did not run `arch_scan`: a fault of the orchestrator):

- T-292: `fleet_runtime/application/pass.rs:3178`, `worktree_custody/application/disk.rs:144`, `worktree_custody/application/reconcile.rs:603`, `ideas/application/sub_orchestrators.rs:699`.
- T-313: `task_execution/application/dispatch.rs:498-644`, `task_execution/adapters/sqlite.rs:205`.
- T-300: `task_execution/application/launch.rs:428`.

## Goal

The fitness test is green with no change of the test, no new entry in an allow list, and no change of behavior or of the statement counts that T-292 and T-313 pinned (their count tests and equivalence tests stay green with no change).

## Steps

1. Run the test and read its full output: each line, the table and the context that owns the table. Read how the test decides the owner of a table, and the architecture document that `architecture_doc` checks (the context map).
2. Read how task T-249 fixed the same fault for `answers.rs` (commit on the run branch: `git log --oneline --grep T-249`): the pattern of this code base (a reader function in the context that owns the table, in its adapter, that the other context calls through the owner's public application or port). Use the same pattern.
3. For each of the 14 lines: move the SQL to a reader of the context that owns the table, with a name that says what it returns. A set read stays a set read: 1 statement, not 1 for each row (the count tests prove it). A join over tables of 2 contexts: decide which context owns the reader by the table that drives the query, and get the other part through a second set read or through a reader that the owner gives; do not add a statement for each row. If 1 join cannot be split without a per-row statement or a change of the result, stop and send `QUESTION:` with the query.
4. `pass.rs` and `reconcile.rs` are not in Own because other tasks edit them (T-286 is in its land for `reconcile.rs`): they are shared files; change only the lines of this ticket there, and rebase before each finish.
5. Run the Check. Then run `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p1_pass` 1 time (the scan test of T-292 must stay green).

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never read, set or pass `ANTHROPIC_API_KEY`.
4. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry and no allow-list entry without an answer of the orchestrator.
5. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet. At most 4 test processes at the same time.
6. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, check that it kept the comments that give a bound, an invariant or a design id; restore them if not.
7. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and the commit message lists each of the 14 lines with its new reader.

## Added 2026-11-21T05:50Z: 1 more reader set of the same file (from task T-286)

Task T-286 landed the step `landings` in `worktree_custody/application/reconcile.rs`. Its worker reports per-row SQL there, which breaks the rule of T-292 (in 1 pass, no SQL statement for each row above `pass_budget` rows; a step that must see all rows reads them in a fixed number of statements): `branch_of` runs 2 statements (the branch `landing_state`, and `max(at)` of its landed event) for each `landing` row; `act` runs `branch_of` again for each `landing` row; `owners` runs `outcome_of` for each `active` row whose owner ended. Bound these 3 in this task, with the method of T-292: a set read in the context that owns the table, 1 count test (the same statement count for 3 and for 300 landing rows; use `taskledger::testing::count_statements`) and 1 equivalence test on a mixed fixture (a landed branch, an abandoned branch, a branch with no landed event, an active row with an ended owner of each outcome). `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs` and `crates/horch-core/tests/worktree_reconcile.rs` are in your scope for this. The new readers must also pass `fitness_no_context_touches_foreign_tables`.

## Added 2026-11-21T06:28Z: a read race of the same readers (from task T-286) — fix it first

`ReadyRows::read` (`task_execution/application/dispatch.rs`, near line 476, from T-313 commit 0b81e22e) reads the ready tasks and the facts of their runs in separate statements. A caller with no transaction can get a ready task whose run facts are not in the second read: `horch ledger status --json` goes `status.rs:608` -> `holds::task_holds` -> `dispatch::choices` -> `ReadyRows::read`, opens no transaction, and fails with `horch: no run <id>` when fleetd makes a task ready between the 2 reads. Evidence: the land Check of T-286 failed 1 time in 7 in `the_whole_flow_on_the_fake_herdr` at a `json_of` call.

The same class of fault can be in each set reader of T-292, T-313 and T-300 that replaced 1 query by several: the old code read row by row and got each row whole; the new code reads 2 or more sets that must be of 1 moment.

1. Write the failing test first, with no sleep and no timing: a test connection reads set 1, then a second connection commits a new ready task, then the reader reads set 2 (give the reader a test hook between its statements, or call its 2 parts from the test). Assert the result that is right: no error, and the new task is either in both sets or in none.
2. Fix by the rule of SQLite: each multi-statement set read runs inside 1 read transaction (a deferred transaction or a savepoint gives 1 snapshot in WAL mode) when the caller has none. Do it in the reader, so that a new caller cannot forget it; a caller that is already in a transaction (the pass) must not get a nested BEGIN: use a savepoint or check `conn.is_autocommit()`. Do NOT fix it by a skip of a task with no run: that hides a torn read.
3. List each multi-statement set reader of the 3 tasks (search their commits) and each caller with no transaction (`status.rs`, `watch`, the read-only commands of the CLI). Apply the same fix; write the list in the commit message.
4. `the_whole_flow_on_the_fake_herdr` is on `crates/horch-e2e/expected-failures/taskledger.txt` (owner ticket 164) for another cause; do not remove it. Add `scripts/dev-sandbox.sh cargo test -p horch --locked --test p6_status` or the test file of `ledger status --json` that exists (find it) to your runs.

## Added 2026-11-21T06:31Z: 1 clippy warning of the same change

`cargo clippy --workspace --all-targets --locked -- -D warnings` fails on the run branch: `clippy::type_complexity` at `crates/horch-core/src/fleetd/worktree_custody/application/disk.rs:190` (from T-292; found by task T-330). Give the type a name (a struct or a type alias with a meaning); no `#[allow]`. Add `cargo clippy --workspace --all-targets --locked -- -D warnings` to your runs before each finish.
