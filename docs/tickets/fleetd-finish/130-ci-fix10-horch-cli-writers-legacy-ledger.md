# CI FIX-10: fix unexplained CLI test failures in horch p5_cli_writers.rs and r_cli_cmds_ledger.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch --test p5_cli_writers --test r_cli_cmds_ledger --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch/tests/p5_cli_writers.rs`, `crates/horch/tests/r_cli_cmds_ledger.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

Run each of the 3 class `f?` tests in `crates/horch/tests/p5_cli_writers.rs` (1 test) and `crates/horch/tests/r_cli_cmds_ledger.rs` (2 tests) 3 times locally through `scripts/dev-sandbox.sh`, find the cause of each failure, and fix it.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- `crates/horch/tests/p5_cli_writers.rs:378-420` and `crates/horch/tests/r_cli_cmds_ledger.rs:1-130`.

## Tests covered (3 class `f?` rows)

1. `crates/horch/tests/p5_cli_writers.rs` (1 test):
   - `triage_archive_is_refused_with_a_later_task` (`crates/horch/tests/p5_cli_writers.rs:403`, Linux + macOS): `assert_eq failed: W1 feat DELETE: queued in the triage_apply job 1; horch worktree triage shows the result`
2. `crates/horch/tests/r_cli_cmds_ledger.rs` (2 tests):
   - `the_legacy_resume_and_assign_arms_run` (`crates/horch/tests/r_cli_cmds_ledger.rs:29`, Linux + macOS): `["add", "--record-id", "r1", "--agent", "claude", "--tier", "t", "--model", "m", "--role", "dev", "--task", "first"]: Output { sta`
   - `the_legacy_subcommands_write_read_and_report_a_session` (`crates/horch/tests/r_cli_cmds_ledger.rs:29`, Linux + macOS): `["add", "--record-id", "r1", "--agent", "claude", "--tier", "t", "--model", "m", "--role", "dev", "--task", "do it"]: Output { sta`

## Steps

1. Run each of the 3 tests 3 times locally through `scripts/dev-sandbox.sh` and inspect the failure:
   - In `crates/horch/tests/p5_cli_writers.rs:378` (`triage_archive_is_refused_with_a_later_task`), `horch worktree triage apply` now queues the `triage_apply` job (`W1 feat DELETE: queued in the triage_apply job 1; horch worktree triage shows the result`) instead of running git synchronously inside the request handler, or refuses `ARCHIVE` with a later task only when the decision is `ARCHIVE`. Update the test to match the Phase 3/5 `triage_apply` job queuing and refusal behavior.
   - In `crates/horch/tests/r_cli_cmds_ledger.rs:29` (`the_legacy_subcommands_write_read_and_report_a_session` and `the_legacy_resume_and_assign_arms_run`), `horch ledger legacy add` writes a session record through `ExecutionStore`, which sends a `session_records` request or queues a request file needing `<fleet_root>/run/operator.token` (or after T-5 / T-899, check how `horch ledger legacy` writes to the store). Seed the operator token / fleet root in `World` of `r_cli_cmds_ledger.rs` so the legacy session subcommands succeed.
2. Never weaken an assert. Run the Check.

## Out of scope

- Any file outside `**Own:**`.

## Done when

All tests in `p5_cli_writers.rs` and `r_cli_cmds_ledger.rs` pass 3 times in a row, and the Check passes.

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

## Changed 2026-11-20

Task T-11 fixed `triage_archive_is_refused_with_a_later_task` (its Check holds `p5_cli_writers`). Check that it is green; your work is the 2 tests of `r_cli_cmds_ledger.rs`.
