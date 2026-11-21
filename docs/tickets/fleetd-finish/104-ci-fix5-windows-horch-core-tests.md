# CI FIX-5: fix the 16 Windows test compile errors in horch-core

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib taskledger::finish::land --locked && scripts/dev-sandbox.sh cargo test -p horch-core --lib taskledger::finish::pr --locked && scripts/dev-sandbox.sh cargo test -p horch-core --lib taskledger::outbox --locked && scripts/dev-sandbox.sh cargo test -p horch-core --lib harness::codex --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/lib.rs`, `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch-core/src/taskledger/finish/pr.rs`, `crates/horch-core/src/taskledger/outbox.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Windows` and `## Fix units` row FIX-5).

## Goal

On Windows CI (`test-windows`), `cargo test --workspace --locked --no-fail-fast` fails to compile the `horch-core` lib test binary with 16 errors across 4 modules (`taskledger::finish::land`, `taskledger::finish::pr`, `harness::codex`, `taskledger::outbox`). You have no Windows host: fix each named compile error by adding the exact `#[cfg(unix)]` gates (or `#[cfg(windows)]` form) below and run the narrow local test of each changed module. The proof on Windows is the next CI run (FIX-7, done by the orchestrator).

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `## Windows` and `## Fix units` row FIX-5.
- The 5 files in `**Own:**` at the line numbers listed below.

## Compile errors covered (16 errors in `horch-core` lib test)

1. `crates/horch-core/src/taskledger/finish/land.rs:1446` and `:1471` (2 errors, class `c`):
   - First error lines:
     - ``error[E0433]: cannot find `unix` in `os` `` (`use std::os::unix::fs::PermissionsExt` at line 1446)
     - ``error[E0599]: no method named `set_mode` found for struct `Permissions` `` (`permissions.set_mode(0o755)` at line 1471)
   - Gate to add: `World::remote` (line 1445) writes a `#!/bin/sh` git `pre-receive` hook and sets its executable mode on unix. Gate `use std::os::unix::fs::PermissionsExt;` and the `permissions.set_mode(0o755)` block (lines 1470-1472) with `#[cfg(unix)]` (or on Windows, where Git for Windows executes `#!/bin/sh` hooks without `PermissionsExt::set_mode`, skip `set_mode` under `#[cfg(unix)]`).
2. `crates/horch-core/src/taskledger/finish/pr.rs:700` and `crates/horch-core/src/lib.rs:72-73` (1 error, class `c`):
   - First error line:
     - ``error[E0433]: cannot find `test_support` in the crate root`` (module `test_support` at `crates/horch-core/src/lib.rs:72-73` has `#[cfg(all(test, unix))]`)
   - Gate to add: In `crates/horch-core/src/taskledger/finish/pr.rs`, `World` (line 665), `World::new` (line 675, calling `crate::test_support::write_warmed` with the `#!/bin/sh` script `FAKE_GH`), `FAKE_GH` (line 644), `no_sleep` (line 655), `NO_WAIT` (line 657), and the 3 tests `trf_50_lagging_pr_head_passes_when_the_remote_branch_holds_the_commit` (line 738), `trf_50_pr_head_catches_up_on_a_later_read` (line 754), and `trf_50_unpushed_commit_still_fails_and_names_the_remote_tip` (line 764) test a unix shell fake `gh`. Gate `FAKE_GH`, `no_sleep`, `NO_WAIT`, `World`, and those 3 tests with `#[cfg(unix)]` (keeping `a_section_is_replaced_in_place_and_others_are_kept` and `new_sections_keep_each_existing_section` cross-platform).
3. `crates/horch-core/src/harness/codex.rs:3312` and `:3417` (2 errors, class `b`):
   - First error line:
     - ``error[E0425]: cannot find function `prepare_in` in this scope`` (2 uses at lines 3312 and 3417; `prepare_in` at line 3147 is gated with `#[cfg(unix)]`)
   - Gate to add: Add `#[cfg(unix)]` to helper `fleet_dirs_with` (line 3303, whose callers `fleet_dirs` at line 3294 and tests are already `#[cfg(unix)]`), and add `#[cfg(unix)]` to `#[test] fn the_pane_tmpdir_is_made_and_stays_under_the_state_root()` (line 3414, which calls `prepare_in`), matching the surrounding `prepare_in` tests in `codex.rs`. Also check `the_private_codex_home_is_outside_every_writable_root` (line 3064) and ensure every caller of `prepare_in` in `codex.rs` has `#[cfg(unix)]`.
4. `crates/horch-core/src/taskledger/outbox.rs:504-538` (11 errors / 13 uses of `libc`, class `c`):
   - First error line:
     - ``error[E0433]: cannot find module or crate `libc` in this scope`` (uses of `libc::pipe`, `fork`, `close`, `read`, `waitpid`, `_exit`, `pid_t`, `c_int` at lines 504-538)
   - Gate to add: `struct Child` (line 503), `impl Child` (line 508), and `#[test] fn trf_21_a_drain_lock_is_free_while_a_child_process_lives()` (line 544) test unix descriptor inheritance across `fork()` (`FD_CLOEXEC`). Add `#[cfg(unix)]` to `struct Child`, `impl Child`, and `fn trf_21_a_drain_lock_is_free_while_a_child_process_lives` (leaving `trf_21_the_board_line_changes_with_the_board` at line 566 cross-platform).

## Steps

1. Inspect the 4 test modules in `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch-core/src/taskledger/finish/pr.rs`, `crates/horch-core/src/harness/codex.rs`, and `crates/horch-core/src/taskledger/outbox.rs`.
2. Add the `#[cfg(unix)]` gates described in items 1 to 4 above so that no item compiled on Windows references `std::os::unix`, `crate::test_support`, `prepare_in`, or `libc` in `outbox.rs`.
3. Run the narrow local tests in the Check and confirm zero unused-item warnings under `cargo test -p horch-core --lib --no-run`.

## Out of scope

- `crates/horch-marketplace/tests/marketplace.rs` (FIX-6).

## Done when

All 4 module test suites in `horch-core` compile and pass without warnings, and the Check passes.

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
