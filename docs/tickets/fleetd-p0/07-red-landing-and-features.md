# 07: Red tests for several ready task branches and 2 features that conflict

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-e2e --test p0_landing --test expected_failures`
**Own:** `crates/horch-e2e/tests/p0_landing.rs`, `crates/horch-e2e/expected-failures/p0_landing.txt`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" items 1 (Phase 0), 6 (Phase 4) and 8 (Phase 6),
and "Features, landing and integration" (Landing a task branch on its feature branch, No work is lost,
Between features).

## Goal

2 scenarios of Phase 0 have a red test: "several task branches ready at the same time, and none is
forgotten" and "2 features that conflict".

## Requirements

No requirement becomes green in Phase 0. This ticket writes the red tests for:

- **Several ready branches** (green in Phase 4): the landing state machine takes the ready task
  branches as 1 bundle. The fast gate runs 1 time for the bundle, and each branch of the bundle lands.
- **2 conflicting features** (green in Phase 6): the trial merge of 2 open features finds the
  conflict. Each feature keeps its tip, and the conflict reaches a decision (the needs-you list or a
  resolver task). No work is lost.

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

- Warning: a land test must never push to a real remote. Use the bare remote that `finish_world` in
  `crates/horch-e2e/tests/taskledger.rs` makes in the harness folder.
- Today each task lands on its own through `horch ledger finish`: `a_task_finishes_through_review_land_pr_and_cleanup`
  and `a_land_checks_outside_the_lock_and_lands_after_a_faster_task` in `taskledger.rs` show the land
  and its check. The land code is `crates/horch-core/src/vcs/landing.rs`. There is no bundle.
- Features, `horch feature`, trial merges and `horch idea add --to` do not exist today. The design
  names them in Phases 4 and 6.

## Steps

1. Write `crates/horch-e2e/tests/p0_landing.rs` with these tests:
   - `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten`: 3 tasks of 1
     run each make 1 commit on its own file, and their 3 finishes start at the same time. The check
     command of the run appends 1 line to a count file each time it runs. Assert that the run branch
     has all 3 commits, and that the count file has 1 line. Today the check runs 3 times.
   - `two_conflicting_features_keep_their_tips_and_reach_a_decision`: 2 ideas go to 2 features
     (`horch idea add` 2 times). Each feature branch changes the same line of 1 file. Assert that
     a trial merge reports the conflict, that both feature tips are unchanged, and that 1 needs-you
     item names both features.
2. Run each test and see it fail. Read the panic message: it must be the absence of the designed
   behavior. Write `crates/horch-e2e/expected-failures/p0_landing.txt`: the first test with phase `4`,
   the second with phase `6`, each with its reason.
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
- Each of the 2 tests is listed with its phase and fails for its listed reason.
- `cargo clippy -p horch-e2e --tests -- -D warnings` passes.
