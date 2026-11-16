# 05: Platform: the fleetd lock and the awake clock

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-platform`
**Own:** `crates/horch-platform/src/lock.rs`, `crates/horch-platform/src/awake.rs`, `crates/horch-platform/src/mac/lock.rs`, `crates/horch-platform/src/linux/lock.rs`, `crates/horch-platform/src/windows/lock.rs`, `crates/horch-platform/src/unix/lock.rs`, `crates/horch-platform/src/mac/awake.rs`, `crates/horch-platform/src/linux/awake.rs`, `crates/horch-platform/src/windows/awake.rs`, `crates/horch-platform/src/unix/awake.rs`, `crates/horch-platform/tests/lock.rs`, `crates/horch-platform/tests/awake.rs`

Spec: `docs/specs/fleetd/design.md`, "Architecture and roles" (One fleetd for each ledger; row Platform layer), "Supervision, liveness and reconciliation" (One fleet clock for each timer: the hang check uses awake time), PL-2, PL-3, PL-5, FM-12.

## Goal

The platform gives 2 fleetd concerns: an exclusive file lock that 1 process holds for its whole life and that the OS frees when the process dies, and an awake clock that does not count sleep and that 2 processes of 1 boot can compare.

## Requirements

- **PL-2** (part): the concerns "the fleetd lock" and "sleep and wake".
- **PL-3** (part): where an OS has no equivalent, the function returns `NotSupported`.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Proposed names: `lock::FleetdLock::try_take(path) -> io::Result<Option<FleetdLock>>` (None when another process holds it; the guard frees it on drop and at process death); `awake::now() -> AwakeInstant` with `boot_id` and `awake_ms`, and `AwakeInstant::elapsed_since(&other) -> Option<Duration>` (None when the 2 readings are of different boots).
- The OS facts: macOS and Linux `flock(LOCK_EX | LOCK_NB)` on an open file (`unix/lock.rs`); Windows `LockFileEx` with `LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY`. Awake time: macOS `mach_absolute_time` (it stops in sleep) with the boot time from `sysctl kern.boottime` as `boot_id`; Linux `CLOCK_MONOTONIC` (it stops in suspend) with `/proc/sys/kernel/random/boot_id`; Windows `QueryUnbiasedInterruptTime` with the boot time from `GetTickCount64` subtracted from the system time, rounded to 1 s.
- Today `crates/horch-core/src/fsx.rs` has `DirLockGuard` (a lock folder with an `owner` file) for the watchdog. The fleetd lock is a different lock: an OS lock that the kernel frees, so a killed fleetd never leaves it held. Do not change `fsx.rs`.
- A test that needs a second process starts the test binary itself as a child with an env flag (a common pattern; see `std::env::current_exe`), never a `horch` binary.

## Steps

1. Write the tests first. Each fails before the code exists:
   - `tests/lock.rs`: `second_take_in_another_process_gets_none`, `lock_is_free_after_the_holder_is_killed` (kill the child with SIGKILL on unix, `TerminateProcess` on Windows), `lock_is_free_after_drop`.
   - `tests/awake.rs`: `awake_clock_moves_forward`, `readings_of_2_processes_of_1_boot_compare`, `readings_of_different_boots_do_not_compare` (build 2 values by hand).
   - A `#[cfg(windows)]` test for each Windows function, run by CI.
2. Write the code. Each `unsafe` block has a `// SAFETY:` note.
3. Run the Check. A Linux or Windows result comes from CI.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes on macOS. The Linux and Windows modules compile in CI. A killed holder frees the lock.
