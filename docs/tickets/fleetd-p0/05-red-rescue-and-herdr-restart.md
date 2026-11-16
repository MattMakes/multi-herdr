# 05: Red tests for a herdr server restart and a worker killed with dirty files

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-e2e --test p0_rescue --test expected_failures`
**Own:** `crates/horch-e2e/tests/p0_rescue.rs`, `crates/horch-e2e/expected-failures/p0_rescue.txt`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" items 1 (Phase 0), 3 (Phase 1) and 5 (Phase 3),
"Worktrees: leased, reconciled, never lost", "Failure modes and effects" rows FM-13 and FM-25, and
`docs/specs/fleetd/preservation-04-behavior.md` rows W6, W8 and W9.

## Goal

2 scenarios of Phase 0 have a red test: "a herdr server restart" (FM-13) and "a worker killed with
dirty and untracked files in its worktree" (FM-25).

## Requirements

No requirement becomes green in Phase 0. This ticket writes the red tests for:

- **FM-13, part of Phase 1** (green in Phase 1): after a herdr restart, the reconcile sees the dead
  processes, ends the attempts and queues the tasks again.
- **FM-13, full** (green in Phase 3): after a herdr restart, each worktree is rescued and the next
  attempt uses it again.
- **FM-25** (green in Phase 3): a worker killed with dirty and untracked files loses nothing. A
  verified rescue ref under `refs/horch/rescue/` holds the work before anything else happens, and
  the worktree stays on disk.

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

- Warning: a rescue test must never touch a real repository. Use `Harness::with_git()` and the git of
  the harness (`h.git_cmd`).
- `crates/horch-e2e/tests/restored_pane.rs` shows a herdr server restart with the fake herdr. A
  restart ends every agent process: the test kills each agent PID and resets the panes of the fake
  herdr as that file does.
- Today a reap leaves the worktree on disk, and there is no rescue ref (`crates/horch-core/src/taskledger/worktree_ops.rs`,
  `crates/horch-core/src/taskledger/watchdog.rs`). The reconcile of today is
  `horch ledger watch --once --force --json`.

## Steps

1. Write `crates/horch-e2e/tests/p0_rescue.rs` with these tests:
   - `fm13_herdr_restart_ends_attempts_and_queues_tasks_again`: 2 workers hold 2 tasks. Restart the
     fake herdr. With the watchdog of today running and no other `horch` call, wait at most 35 s.
     Assert that each attempt is ended and each task is queued again.
   - `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it`: 1 worker holds a task
     with a commit and an uncommitted change in its ledger worktree. Restart the fake herdr, and
     reconcile with `horch ledger watch --once --force --json`. Assert that a ref under
     `refs/horch/rescue/` holds the work, and that the next claim of the task gets the same worktree.
   - `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing`: a worker claims and makes its
     ledger worktree. It changes 1 tracked file and writes 1 untracked file, with no commit. Kill the
     agent with `SIGKILL`, then reconcile until the reap (`--stall-rounds 1`). Assert that a ref under
     `refs/horch/rescue/` exists, that its tree holds both files with the same bytes, and that the
     worktree folder is still on disk.
2. Run each test and see it fail. Read the panic message: it must be the absence of the designed
   behavior. Write `crates/horch-e2e/expected-failures/p0_rescue.txt`: the first test with phase `1`,
   the other 2 with phase `3`, each with its reason.
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
- Each of the 3 tests is listed with its phase and fails for its listed reason.
- `cargo clippy -p horch-e2e --tests -- -D warnings` passes.
