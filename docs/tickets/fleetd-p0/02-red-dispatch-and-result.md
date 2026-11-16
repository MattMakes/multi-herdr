# 02: Red tests for a worker that never claims and a worker that exits without done

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-e2e --test p0_dispatch --test expected_failures`
**Own:** `crates/horch-e2e/tests/p0_dispatch.rs`, `crates/horch-e2e/expected-failures/p0_dispatch.txt`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 1 (Phase 0) and item 6 (Phase 4), and
"Failure modes and effects", rows FM-2 and FM-6.

## Goal

2 scenarios of Phase 0 have a red test: "an agent that never claims" (FM-2) and "one that exits
without `done`" (FM-6). Each test is on the list of expected failures with phase 4.

## Requirements

No requirement becomes green in Phase 0. This ticket writes the red tests for:

- **FM-2** (green in Phase 4): push dispatch. fleetd binds the task to the attempt and writes
  `task.yaml` before the launch. There is no claim.
- **FM-6** (green in Phase 4): a worker that ends without a result does not lose its work. The next
  attempt reuses the same worktree.

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

- Today a worker gets its task only through `horch ledger claim` (`crates/horch/src/cmd/taskledger.rs`
  and `crates/horch-core/src/taskledger/`). `queue_claim_and_status_through_the_cli` and
  `three_checks_without_progress_reap_the_worker_by_its_pid` in `crates/horch-e2e/tests/taskledger.rs`
  show how a test queues a task, spawns a worker with the fake scenario `exec,stay` and reads
  `horch ledger status --json` and `horch ledger show <id> --json`.

## Steps

1. Write `crates/horch-e2e/tests/p0_dispatch.rs` with these tests:
   - `fm2_agent_that_never_claims_has_its_task_bound_at_spawn`: queue 1 task, run `horch spawn` for
     its persona with the fake scenario `exec,stay` (the agent never runs `horch ledger claim`).
     Assert that, before any claim, the task is bound to the new worker in `horch ledger status
     --json`, and that a `task.yaml` file for the attempt exists (design, "Workspace layout and file
     protocol"). Today it fails: the task stays queued until a claim.
   - `fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt`: a worker claims, makes
     a commit in its ledger worktree (`horch ledger worktree new`, as `work_and_finish` in
     `taskledger.rs` does), and its agent process ends with no `done` and no result. Reconcile with the
     path of today (`horch ledger watch --once --force --json` on a run with `--stall-rounds 1`).
     Assert that the task is queued for a next attempt, and that the next claim gets the same
     worktree path with the commit in it. Record in the list the reason that you see today.
2. Run each test and see it fail. Read the panic message: it must be the absence of the designed
   behavior. Then write `crates/horch-e2e/expected-failures/p0_dispatch.txt` with 1 line for each test,
   phase `4`, and that reason.
3. Run the Check. It passes when each listed test fails for its reason and the list audit passes.

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
- Each of the 2 tests is listed with phase `4` and fails for its listed reason, not for a fault of the test.
- `cargo clippy -p horch-e2e --tests -- -D warnings` passes.
