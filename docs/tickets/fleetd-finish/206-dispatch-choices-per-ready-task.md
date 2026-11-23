# A large ready queue: the dispatch step runs SQL for each ready task above the pass budget

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test push_dispatch && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test fleetd_pass && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p1_pass && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/tests/push_dispatch.rs`

Source: the note of task T-292 (worker opus-61, 2026-11-21T03:50Z).

## Fault

Task T-292 gave the pass this rule (design A32: nothing slow runs between 2 heartbeats; each pass has a budget): in 1 pass, no SQL statement for each row above `pass_budget` rows; a step that must see all rows reads them in a fixed number of statements. 2 readers still break the rule when the READY queue is large: `dispatch::choices` runs SQL for each ready task (`cancel_requested`, `parked_idea`, `get`, `task_tree`, `files`, `open_attempt`), and `read_base_tips` does the same. The scan test of T-292 has 0 ready tasks of 10000, so it does not see this.

## Goal

With 10000 ready tasks, 1 pass keeps the rule, and the dispatch result is the same as today for each fixture.

## Steps

1. Read the commits of T-292 on the run branch: the count tests and the helper `taskledger::testing::count_statements`. Use the same method.
2. Decide for each reader by its meaning: a pass can launch only as many tasks as there are free slots of the personas, so the step needs the first N startable tasks in the designed order (priority, then age: read the order rule in `dispatch.rs` and the design), not each ready task. Either read the candidates in the designed order with a limit and a fixed number of statements, or read all in a fixed number of statements. Do not change the order of dispatch, the Own-files rule of a bind, or which task is skipped and why.
3. Write the failing count test first: N = 30 and N = 3000 ready tasks give the same number of statements (or at most `pass_budget` row statements).
4. Write the equivalence test: on a mixed fixture (a cancelled task, a task of a parked idea, a task with an open attempt, 2 tasks with the same Own file, 2 personas with 1 free slot each) the choices are the same as before the change. Record the result of the old code in the test as fixed values, from a run before you change the code.
5. Run `p1_pass` 3 times.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never read, set or pass `ANTHROPIC_API_KEY`.
4. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry without an answer of the orchestrator.
5. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet. At most 4 test processes at the same time.
6. `dispatch.rs` and `push_dispatch.rs` are shared files: `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`.
7. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes, and the count test and the equivalence test are green 3 times.
