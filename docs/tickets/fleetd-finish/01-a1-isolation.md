# 01: A1 isolation: dev-sandbox script, shared-ledger write guard, service label, e2e seal, and old-watchdog stop proof

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib taskledger::store && scripts/dev-sandbox.sh cargo test -p horch-core --test old_watchdog && scripts/dev-sandbox.sh cargo test -p horch-platform --test service && scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::fleetd && cargo fmt --all --check`
**Own:** `.gitignore`, `scripts/dev-sandbox.sh`, `crates/horch-core/src/taskledger/store.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/old_watchdog.rs`, `crates/horch-core/tests/old_watchdog.rs`, `crates/horch-platform/src/service.rs`, `crates/horch-platform/tests/service.rs`, `crates/horch/src/cmd/fleetd.rs`, `crates/horch-e2e/src/harness.rs`
**Do not touch:** `docs/specs/fleetd/design.md`

Spec: `ai_docs/plans/fleetd-finish/plan.md` (section 1 items L1, L2, L3; section 3 unit A1) and `ai_docs/plans/fleetd-finish/audit/isolation.md`.

## Goal

A development build cannot write the live shared ledger or collide with the installed service label. Every test and binary run uses `scripts/dev-sandbox.sh` with isolated directories inside the worktree.

## Requirements

- **L1**: `Store::open` in `crates/horch-core/src/taskledger/store.rs` refuses every writable open of the shared ledger `<passwd_home>/.horch/ledger.sqlite3` by a development binary before `create_dir_all` and `Connection::open`, unless `HORCH_ALLOW_LEDGER_MIGRATE=1`.
- **L2**: `crates/horch-platform/src/service.rs` and `crates/horch/src/cmd/fleetd.rs` keep `horch-fleetd` for the default ledger only. A non-default ledger derives a unique service label from the canonical ledger path or takes a validated label override.
- **L3**: `Harness::seal` in `crates/horch-e2e/src/harness.rs` sets `HORCH_LEDGER_DB` to `<Harness::root>/ledger/ledger.sqlite3` (outside `HORCH_STATE_DIR`) and leaves `FLEET_ROOT` unset.

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

1. Add `.dev-sandbox/` to `.gitignore`. Write `scripts/dev-sandbox.sh` with mode `0755`. The script takes `<command...>`, creates `<worktree>/.dev-sandbox/run.$$` with subdirectories `home`, `tmp`, `state`, `data`, `ledger`, `config`, and `cache`, sets a `trap` on `EXIT INT TERM` that removes the run folder, unsets `FLEET_ROOT`, `HORCH_ALLOW_LEDGER_MIGRATE`, and `ANTHROPIC_API_KEY`, preserves `CARGO_HOME` and `RUSTUP_HOME`, exports `HOME`, `TMPDIR`, `HORCH_STATE_DIR`, `HORCH_DATA_DIR`, `HORCH_LEDGER_DB`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `CODEX_HOME`, and `CLAUDE_CONFIG_DIR` as in `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", and runs `"$@"`.
   - Check: `scripts/dev-sandbox.sh sh -c 'test -z "$FLEET_ROOT" && test -z "$ANTHROPIC_API_KEY" && test -n "$HORCH_LEDGER_DB"'` exits 0 and leaves no `.dev-sandbox/run.*` folder.
2. In `crates/horch-core/src/taskledger/store.rs`, add a writable-open guard function `write_refusal` and call `guard_open(path)` at the top of `Store::open` (line 178), before `std::fs::create_dir_all` and `Connection::open`. A development binary (`is_dev_build(&exe)`) that opens the shared ledger (`is_shared_ledger(ledger, passwd_home)`) fails unless `HORCH_ALLOW_LEDGER_MIGRATE=1`, even when the schema version already matches `schema::VERSION`. Add unit tests in `crates/horch-core/src/taskledger/store.rs` that prove a dev build refuses a same-version writable open of the shared ledger and permits a scratch ledger or `HORCH_ALLOW_LEDGER_MIGRATE=1`.
   - Check: `scripts/dev-sandbox.sh cargo test -p horch-core --lib taskledger::store`
3. In `crates/horch-platform/src/service.rs` and `crates/horch/src/cmd/fleetd.rs` (`service_spec` at line 439), derive the service label from the canonical ledger path for a non-default ledger (`horch-fleetd-<8 hex chars of sha256>`) or accept a validated label override (`validate_label`), and keep `service::LABEL` (`horch-fleetd`) for the default ledger only. Add unit and integration tests in `crates/horch-platform/tests/service.rs` and `crates/horch/src/cmd/fleetd.rs` for both the default ledger label and a non-default ledger label.
   - Check: `scripts/dev-sandbox.sh cargo test -p horch-platform --test service && scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::fleetd`
4. In `crates/horch-e2e/src/harness.rs`, update `Harness::seal` (around line 367) and `Harness::ledger_db` (around line 446) so that `Harness::seal` sets `HORCH_LEDGER_DB` to `self.root.join("ledger").join("ledger.sqlite3")` (outside `self.state`) by default when `self.env` does not override it and `self.removed` does not remove it. Leave `FLEET_ROOT` unset by default.
   - Check: `scripts/dev-sandbox.sh cargo check -p horch-e2e --tests`
5. Review `take_over` in `crates/horch-core/src/fleetd/fleet_runtime/adapters/old_watchdog.rs` (lines 69 to 250). Add a test in `crates/horch-core/tests/old_watchdog.rs` that records a watchdog PID in ledger A and another live PID in ledger B, runs `take_over` on ledger A, and proves that `fleetd` stops only the PID recorded in ledger A with its matching start time.
   - Check: `scripts/dev-sandbox.sh cargo test -p horch-core --test old_watchdog`
6. Run the full ticket Check and verify that `ps` shows no leftover child process and `.dev-sandbox/` is removed.

## Out of scope

- Changing the fleet-root default in `crates/horch-core/src/runtime/paths.rs` or `docs/specs/fleetd/design.md` (design question Q4).
- Running the full workspace e2e test suite (unit B1 runs it).

## Done when

The Check passes, `scripts/dev-sandbox.sh` removes its sandbox directory on exit and on signal, `Store::open` refuses a dev-build writable open of the shared ledger before touching disk, non-default ledgers get a distinct service label, `Harness::seal` sets `HORCH_LEDGER_DB` under `Harness::root`, and the old-watchdog test passes.
