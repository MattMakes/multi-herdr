# CI FIX-10: fix unexplained unit test failures in horch-core codex.rs, launch.rs and schema.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --lib harness::codex::tests::each_launch_gets_only_its_own_rules harness::launch::tests::an_agent_launch_drops_the_git_repository_variables taskledger::schema::v7_tests::trf_48_every_old_version_migrates_to_current --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/harness/launch.rs`, `crates/horch-core/src/taskledger/schema.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

Run each of the 3 class `f?` unit tests in `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/harness/launch.rs`, and `crates/horch-core/src/taskledger/schema.rs` 3 times locally through `scripts/dev-sandbox.sh`, find the cause of each failure, and fix it.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- `crates/horch-core/src/harness/codex.rs:2743-2768`, `crates/horch-core/src/harness/launch.rs:3118-3180`, and `crates/horch-core/src/taskledger/schema.rs:2264-2295`.

## Tests covered (3 class `f?` rows)

1. `harness::codex::tests::each_launch_gets_only_its_own_rules` (`crates/horch-core/src/harness/codex.rs:2761`, Linux + macOS): `assertion failed: read(&orchestrator).contains(r#"pattern = ["horch", "spawn"]"#)`
2. `harness::launch::tests::an_agent_launch_drops_the_git_repository_variables` (`crates/horch-core/src/harness/launch.rs:3134`, Linux): `` (empty first line; child test `git_env_agent_launch_child` failed on Linux when `socat` was missing or when launching `opus`)
3. `taskledger::schema::v7_tests::trf_48_every_old_version_migrates_to_current` (`crates/horch-core/src/taskledger/schema.rs:2279`, Linux + macOS): `assert_eq failed: version 14`

## Steps

1. Run each of the 3 tests 3 times locally through `scripts/dev-sandbox.sh` and inspect the failure:
   - In `crates/horch-core/src/harness/codex.rs:2761` (`each_launch_gets_only_its_own_rules`), Phase 6 retired `horch spawn` from `roster.orchestrator_exec_rules()` (or changed its decision / rule pattern). Update the assertion to check a rule that differentiates `orchestrator_exec_rules()` from `exec_rules()` under Phase 6.
   - In `crates/horch-core/src/harness/launch.rs:3118` (`an_agent_launch_drops_the_git_repository_variables`), check `git_env_agent_launch_child` (line 3173) when `host_problem` reports missing `bwrap` or `socat` on Linux, or ensure the child test handles a host without `bwrap`/`socat` consistently.
   - In `crates/horch-core/src/taskledger/schema.rs:2279` (`trf_48_every_old_version_migrates_to_current`), inspect the diff between `schema_text(store.conn())` after migrating from `version 14` (`PHASE_6_VERSION`) to `VERSION` (`15`, `PHASE_7_VERSION`) and `schema_text(fresh.conn())` (for example a column order, index, or table definition difference between fresh `CREATE TABLE` and the `v14 -> v15` migration step in `schema.rs`), and fix the migration or fresh schema in `crates/horch-core/src/taskledger/schema.rs` so `schema_text(store.conn()) == want` holds for every version `1..VERSION`.
2. Never weaken an assert. A flaky test is a bug.
3. Run the Check (3 iterations of all 3 tests plus `cargo fmt --all --check`).

## Out of scope

- Any file outside `**Own:**`.

## Done when

All 3 unit tests pass 3 times in a row, and the Check passes.

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
