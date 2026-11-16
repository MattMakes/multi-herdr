# 04: Red tests for a killed watchdog, a restart on a new binary and a hung fleetd

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-e2e --test p0_supervision --test p0_fleetd_hang --test expected_failures`
**Own:** `crates/horch-e2e/tests/p0_supervision.rs`, `crates/horch-e2e/tests/p0_fleetd_hang.rs`, `crates/horch-e2e/expected-failures/p0_supervision.txt`, `crates/horch-e2e/expected-failures/p0_fleetd_hang.txt`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" items 1 (Phase 0) and 3 (Phase 1), "Supervision,
liveness and reconciliation", "Failure modes and effects" rows FM-7, FM-12 and FM-34, and
`docs/specs/fleetd/preservation-04-behavior.md` row S10 and
`docs/specs/fleetd/preservation-06-wiring-and-platform.md` item S14 of its Summary (amendment A21 makes it a row).

## Goal

3 scenarios of Phase 0 have a red test: "a killed watchdog" (FM-7), "a daemon restart on a new binary
in the middle of a run" (FM-34), and "a hung fleetd that is restarted, on each OS" (FM-12).

## Requirements

No requirement becomes green in Phase 0. This ticket writes the red tests for:

- **FM-7** (green in Phase 1): 1 supervised daemon. A level-triggered pass every 5 s acts in code.
- **FM-34** (green in Phase 1): `horch self-update` or a changed binary restarts the running daemon
  on the new binary. The run goes on with 1 attempt for each task (row S10 of today stays).
- **FM-12** (green in Phase 1): when the heartbeat of fleetd is older than 30 s of awake time,
  `horch fleetd check` (the job that the service manager of each OS runs every 30 s) stops it by its
  PID and start time and starts a new one (PL-5, row 06 S14).

## Context

- Ticket 01 gives `horch_e2e::expected::check(file!(), "<test name>", || { ... })` and the list
  format `<test name> | <phase> | <reason>` in `crates/horch-e2e/expected-failures/README.md`. Read
  that README first.
- Each red test wraps its whole body in `check`. Each red test has 1 line in the list file of this
  ticket, with the phase that makes it green (the table "Where each rule becomes green" of the
  design) and the reason for which it fails today.
- The reason is the absence of the designed behavior: for example the text of a failed assertion,
  or the error of a command that does not exist yet. A reason that is a fault of the test or of the
  harness (a timeout of `wait_for` on a setup step, a missing fake, a compile error) is wrong.
- Each assertion message contains the stdout and stderr of the command that it checks, so that the
  reason text is in the panic message.
- A test asserts the behavior of the design, with the numbers of the design (the table "The numbers
  of the design"). Where the design names a command that does not exist yet (`horch fleetd`,
  `horch fleetd check`, `horch idea add`, `horch worktrees`), call it as the design names it.
- Use `horch_e2e::harness::Harness` and the existing fake scenarios. Do not change the fakes in
  `crates/horch-e2e/src/bin/` or the harness: they are not in `Own`. The test plays the agent where
  a fake cannot, as `crates/horch-e2e/tests/taskledger.rs` (`three_checks_without_progress_reap_the_worker_by_its_pid`,
  `watched_run`) and `crates/horch-e2e/tests/watchdog_liveness.rs` do: it runs worker commands with
  `HORCH_WORKER_ID`, starts a `sleep` process as the agent, and kills PIDs.
- Copy the small helpers (`text`, `ok`, `json_of`, `ticket`, `fleet`) from `taskledger.rs` into the
  new test file. Do not import them from another test file.
- A test must end in less than 2 min. Where a timer of the design is long (5 min, 7 min, 10 min), the
  test checks the timer through its value instead of a wait (for example the fields `check_every_s`
  and `stall_rounds` of `Run` in `crates/horch-core/src/taskledger/runs.rs`, read from the ledger of
  the harness).
- Start the test file with `#![cfg(unix)]`, as the existing e2e test files do, unless a step says
  otherwise. The Windows result comes from CI after Phase P.

- Warning: never start a service unit, a launchd job or a scheduled task. The tests stand in for the
  service manager: they call `horch fleetd check` themselves, as the 30 s timer of the design does.
- Each `horch fleetd` process that a test starts uses the temporary folders of the `Harness`
  (`HORCH_STATE_DIR` and the ledger in `h.root`). The test stops it at the end, also when the test
  fails (a guard with `Drop`, as `Watchdog` in `crates/horch-e2e/tests/watchdog_liveness.rs` does).
- Today the run's watchdog starts lazily from `ensure` on a ledger command, and it restarts itself when
  its binary changes: `a_watchdog_of_another_build_is_stopped_by_pid_and_replaced` and `install_copy`
  in `watchdog_liveness.rs`, `a_dead_watchdog_is_restarted_and_a_second_one_is_refused` in
  `taskledger.rs`.
- `HORCH_FAULT` arms fault points (`crates/horch-core/src/runtime/fault.rs`). The design does not name
  the fault point of a hung pass. Use the name `hang-pass`. Phase 1 adds it to fleetd.

## Steps

1. Write `crates/horch-e2e/tests/p0_supervision.rs` with these tests:
   - `fm7_killed_watchdog_does_not_stop_the_reconcile`: a run with its watchdog started (`watched_run`
     in `taskledger.rs` shows how). Kill the watchdog with `SIGKILL`. Then kill the agent of a claimed
     task. With no `horch` call other than `horch fleetd check` (the timer of the design), wait at
     most 35 s. Assert that the task is queued again. Today the reason is that `horch fleetd` does not
     exist and nothing starts the watchdog again.
   - `fm34_daemon_restart_on_a_new_binary_keeps_the_run`: start `horch fleetd` from a copy of the
     binary (`install_copy`). A worker holds a task. Replace the copy with a new mtime. Assert that,
     within 2 passes (10 s), a new fleetd process runs from the new binary, the old one is gone, and
     the task still has exactly 1 attempt.
2. Write `crates/horch-e2e/tests/p0_fleetd_hang.rs` with 1 test,
   `fm12_hung_fleetd_is_restarted_on_each_os`. Warning: this file has no `#![cfg(unix)]`, because the
   design wants the result on macOS, Linux and Windows. Use only `std` and `horch_core` functions
   that compile on each OS (`horch_core::procid::alive`). Start `horch fleetd` with
   `HORCH_FAULT=hang-pass`. Wait until its heartbeat is older than 30 s, then run `horch fleetd check`.
   Assert that the hung process is gone, a new fleetd runs, and its heartbeat is fresh.
3. Run each test and see it fail. Read the panic message: it must be the absence of the designed
   behavior. Write the 2 list files, each test with phase `1` and its reason.
4. Run the Check. This Mac cannot build for Windows (no `rustup` targets). CI gives the Windows
   result, after Phase P makes the Windows test job pass. Say so in your summary.

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

- The Check passes.
- Each of the 3 tests is listed with phase `1` and fails for its listed reason.
- No `horch fleetd` or watchdog process of a test is alive after the Check (`pgrep -fl 'horch fleetd'`
  shows none from the harness folders).
- `cargo clippy -p horch-e2e --tests -- -D warnings` passes.
