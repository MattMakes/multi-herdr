# CI FIX-10: fix unexplained unit test failures in horch cmd/context.rs, cmd/messaging.rs, cmd/taskledger.rs and main.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::context::tests::ctx_06_windows_view_lists_claude_and_codex_teammates cmd::messaging::tests::trf_45_note_and_ledger_progress_are_1_command_and_write_no_json cmd::taskledger::tests::no_allowed_ledger_subcommand_takes_a_check_command tests::help_lists_the_same_commands_in_the_same_order --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/context.rs`, `crates/horch/src/cmd/messaging.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch/src/main.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows 8 and 10).

## Goal

Run each of the 3 class `f?` unit tests in `crates/horch/src/cmd/context.rs`, `crates/horch/src/cmd/messaging.rs`, and `crates/horch/src/cmd/taskledger.rs` (plus `tests::help_lists_the_same_commands_in_the_same_order` in `crates/horch/src/main.rs` from `red-baseline.txt`) 3 times locally through `scripts/dev-sandbox.sh`, find the cause of each failure, and fix it.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- `crates/horch/src/cmd/context.rs:1238-1285`, `crates/horch/src/cmd/messaging.rs:1457-1545`, `crates/horch/src/cmd/taskledger.rs:2124-2160`, and `crates/horch/src/main.rs:1877-1910`.

## Tests covered (3 class `f?` rows + 1 `red-baseline.txt` row)

1. `cmd::context::tests::ctx_06_windows_view_lists_claude_and_codex_teammates` (`crates/horch/src/cmd/context.rs:1267`, Linux + macOS, class `f?` and in `red-baseline.txt`): `codex-sol                    codex     gpt-5.6-sol                           256,000  fleet     256,000     160,000  teammate    1`
2. `cmd::messaging::tests::trf_45_note_and_ledger_progress_are_1_command_and_write_no_json` (`crates/horch/src/cmd/messaging.rs:1532`, Linux + macOS, class `f?`): `Err: refused: not handled in this version`
3. `cmd::taskledger::tests::no_allowed_ledger_subcommand_takes_a_check_command` (`crates/horch/src/cmd/taskledger.rs:2151`, Linux + macOS, class `f?`): `["reopen"]`
4. `tests::help_lists_the_same_commands_in_the_same_order` (`crates/horch/src/main.rs:1877`, in `red-baseline.txt`): `assertion left == right failed   left: [("channel", true), ("mail", true), ("fleet", false), ...`

## Steps

1. Run each of the 4 tests 3 times locally through `scripts/dev-sandbox.sh` and inspect the failure:
   - In `crates/horch/src/cmd/context.rs:1267` (`ctx_06_windows_view_lists_claude_and_codex_teammates`), Phase 7 ticket 21 (`50cc5eda`, FR-9) set `compact_window` of every teammate to `256,000` (`fleet`). Update the expected table column assertions in `ctx_06_windows_view_lists_claude_and_codex_teammates` to match the Phase 7 256k context window rule (FR-9).
   - In `crates/horch/src/cmd/messaging.rs:1532` (`trf_45_note_and_ledger_progress_are_1_command_and_write_no_json`), inspect why `note` / `ledger progress` returned `Err: refused: not handled in this version` after T-897 / Phase 5 outbox and channel routing, and update the test setup (such as attempt outbox `HORCH_TASK_DIR` or operator channel handler) to match the Phase 5/6 progress flow.
   - In `crates/horch/src/cmd/taskledger.rs:2151` (`no_allowed_ledger_subcommand_takes_a_check_command`), `horch ledger reopen` was retired in Phase 6 (`reopen` still has a hidden `--check` arg on the retired clap struct while not listed as `Forbidden` in `roster.exec_rules()`, or `takes_check` matches retired subcommands). Remove the `--check` flag from the retired `Reopen` clap variant in `crates/horch/src/cmd/taskledger.rs` so no allowed subcommand takes `--check`.
   - In `crates/horch/src/main.rs:1877` (`tests::help_lists_the_same_commands_in_the_same_order`), verify that it passes 3 times in a row.
2. Never weaken an assert. Run the Check.

## Out of scope

- Any file outside `**Own:**`.

## Done when

All 4 unit tests in `crates/horch` pass 3 times in a row, and the Check passes.

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
