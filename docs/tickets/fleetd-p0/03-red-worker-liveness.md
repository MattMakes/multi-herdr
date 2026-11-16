# 03: Red tests for an idle claim, a dead agent, a usage limit and a machine sleep

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-e2e --test p0_liveness --test expected_failures`
**Own:** `crates/horch-e2e/tests/p0_liveness.rs`, `crates/horch-e2e/expected-failures/p0_liveness.txt`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" items 1 (Phase 0), 3 (Phase 1) and 4 (Phase 2),
"Failure modes and effects" rows FM-3, FM-4 and FM-5, and "The numbers of the design".

## Goal

4 scenarios of Phase 0 have a red test: "one that claims and stops" (FM-3), an agent process that
dies silently (FM-4), "a usage-limited worker that must not be reaped" (FM-5), and "a machine sleep
that must not expire a lease".

## Requirements

No requirement becomes green in Phase 0. This ticket writes the red tests for:

- **FM-3** (green in Phase 2): an idle claim is caught in under 10 min (the gate test of Phase 2).
  The design catches it in about 7 min: lease 120 s, stall 300 s.
- **FM-4** (green in Phase 1): a dead agent is caught in 5 to 30 s.
- **FM-5** (green in Phase 2): a usage-limited worker is not reaped and burns no attempt. Its task
  moves to the fallback persona of another provider, or waits in `waiting_quota` with its lease paused.
- **Machine sleep** (green in Phase 2): the lease is on the fleet clock of awake time, so a sleep does
  not expire it.

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

- Today the watchdog of a run checks every `check_every_s` (default 600 s) and reaps after
  `stall_rounds` (default 3) checks with no progress (`crates/horch-core/src/taskledger/runs.rs`,
  `crates/horch-core/src/taskledger/watchdog.rs`). The design replaces "3 checks of 600 s".
- `HORCH_NOW=<RFC 3339>` pins the clock of 1 `horch` process (`crates/horch-core/src/clock.rs`). It
  stands for the wall clock after a sleep.
- `HORCH_QUOTA_FILE` points at a quota fixture. `crates/horch-core/tests/fixtures/telemetry/quota/claude-exhausted-codex-ok.json`
  gives an exhausted Claude pool and a free Codex pool. `horch_e2e::harness::fixtures()` returns that
  folder's parent.
- `watched_run` in `crates/horch-e2e/tests/taskledger.rs` starts the real watchdog of a run
  (`h.unset("HORCH_NO_WATCHDOG")`) and records an agent PID with
  `horch_core::taskledger::supervisor::agent_started`. `horch_core::procid::alive` checks a PID.

## Steps

1. Write `crates/horch-e2e/tests/p0_liveness.rs` with these tests:
   - `fm3_idle_claim_is_caught_in_under_10_min`: make a run with the default policy. Read its
     `check_every_s` and `stall_rounds` from the ledger of the harness. Assert that the time from the last
     activity to the reap is less than 600 s. Today it is 1800 s.
   - `fm4_dead_agent_ends_its_attempt_within_30_s`: a run with its watchdog started and the default
     policy. A worker claims, and the test kills its agent process (`SIGKILL`). With no other `horch`
     call, wait at most 35 s. Assert that the task is queued again. Today nothing checks before 600 s.
   - `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt`: a Claude worker claims, with the
     quota fixture `claude-exhausted-codex-ok.json`. Run the checks of today until a reap is due
     (`--check-every 1 --stall-rounds 1` and `horch ledger watch --once --force --json`). Assert that
     the agent is not reaped as dead and that the attempt count of the task did not grow. Assert that
     the task moves to the Codex fallback or waits in `waiting_quota`.
   - `sleep_does_not_expire_a_lease`: a worker claims, and its agent stays alive. Run 1 check with
     `HORCH_NOW` set 2 h after the claim. Assert that the task has a lease with its time left, as the
     design's `leases` table gives it. Today the task has no lease.
2. Run each test and see it fail. Read the panic message: it must be the absence of the designed
   behavior. Write `crates/horch-e2e/expected-failures/p0_liveness.txt`: `fm4_...` with phase `1`, the
   other 3 with phase `2`, each with the reason that you see.
3. Run the Check.

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
- Each of the 4 tests is listed with its phase and fails for its listed reason.
- `fm4_dead_agent_ends_its_attempt_within_30_s` leaves no process: the harness teardown or the
  test stops each process that it started.
- `cargo clippy -p horch-e2e --tests -- -D warnings` passes.
