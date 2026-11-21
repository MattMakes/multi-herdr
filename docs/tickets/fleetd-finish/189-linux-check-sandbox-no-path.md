# Linux: fleetd finds no bwrap for the check sandbox, so each persona that is not codex is held (`no_check_sandbox`), also in production

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test check_sandbox && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked harness::claude && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p7_exec && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs`, `crates/horch-core/tests/check_sandbox.rs`, `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch-core/src/harness/claude.rs`

## Fault

Found by task T-246 (opus-51) on 2026-11-20, in a Linux container.

- `WorkerSandboxes::new` at `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs:82` calls `host_problem(None)`.
- On Linux, `None` means "no PATH" (pinned by a test near `crates/horch-core/src/harness/claude.rs:3037`), so `find_bwrap` always fails.
- Result: on each Linux host, fleetd holds each persona that is not codex with `no_check_sandbox`. The log line is `opus: sandbox is not supported on linux: bwrap is not on PATH`. This is so in production too, not only in tests. macOS ignores the path, so macOS does not show it.
- 2 tests mirror the fault: `host_gap()` in `crates/horch-core/tests/check_sandbox.rs:292` and a test near `crates/horch/src/cmd/taskledger_watch.rs:507`.
- Evidence: `crates/horch-e2e/tests/p7_exec.rs` fails 7 of 8 in the container after the fixes of T-240 and T-246.

## Goal

On Linux, fleetd finds `bwrap` and `socat` when they are installed, and holds a persona only when they are not. The rule of SR-4 stays: no check and no worker runs without its sandbox; a host with no `bwrap` still holds the persona with the same reason.

## Steps

1. Decide where the search path comes from, and write the reason in the commit message. This is a security choice: a `bwrap` found through a `PATH` that a worker can change would let a worker replace the sandbox tool. The path must be the one of the fleetd process at its start (the operator's), never a value from an attempt, a profile of a worker, or a request. If the code already has a trusted tool lookup for the launcher sandbox, use the same one, so that 1 rule finds the tool for both.
2. Write the failing test first: with a search path that holds a fake `bwrap` and `socat`, `WorkerSandboxes::new` reports no host problem on Linux; with a path that holds none, it reports the hold. Make the tests run on macOS too where the code permits (the path logic is plain code), so that the Check proves them here.
3. Fix `check_sandbox.rs:82`. Correct the 2 mirror tests so that they test the rule, not the fault.
4. Linux proof: use the container method of T-240 and T-246 (ask opus-51 for the exact command, or read the DONE notes of T-240 and T-246). The image needs git 2.41 or later for other tests; for `p7_exec` the image `rust:1-bookworm` was enough. Run `p7_exec` there 3 times.
5. Docker rules: this host runs services of the operator in docker (`litellm-postgres`, `phoenix`, `buzz-prod-*`). Never stop, restart, remove or prune them; never run a `docker ... prune` command; use `docker run --rm`; remove by exact name only what you made or pulled (container, volume, image) before DONE, and write `docker ps -a`, `docker images`, `docker volume ls` results for your items in the DONE note. If docker does not work, write "not proved on Linux, CI proves it".

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build or a tool. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. No flaky test. Do not weaken an assert. No `#[ignore]`.
6. Never run `rm -rf /tmp/horch-e2e-*`. Remove only the exact folder of your own test.
7. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes, the tests of step 2 pass 3 times, and `p7_exec` passes 3 times on Linux (or the DONE note says that Linux is not proved).
