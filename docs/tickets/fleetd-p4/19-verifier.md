# 19: The verifier: a task in verifying gets its check as a task_check job, verdict.json is written, and only the verifier moves a task to done

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --test verifier && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/verification.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/verdict_file.rs`, `crates/horch-core/tests/verifier.rs`, `crates/horch-core/tests/verifier_compile_fail.rs`

Spec: `docs/specs/fleetd/design.md`, "Architecture and roles" (Verifier; Done for each kind of task), "The task state machine" (`verifying`), RR-3 (the Phase 4 part), FM-9 (the part test), "The kinds of job" (`task_check`: pure, keyed by the tip and the hash of the check).

## Goal

When a push attempt's task is `verifying`, fleetd queues a `task_check` job keyed by the tip of the task branch and the hash of the check, and never waits for it. A later pass reads the job: a pass starts the review steps (ticket 20) or, for a judgment kind, the structural check (ticket 21); a fail ends the attempt with the outcome `rejected` (rung 6). fleetd writes `verdict.json` of the task. Only the verifier makes the value that moves a task to `done`.

## Requirements

- RR-3 (Phase 4 part): a task reaches `done` only when its result file exists, validates, and its check passes when run by the supervisor.
- `verified_verdict_has_no_constructor_outside_the_verification_module`: a compile-fail test (a `trybuild`-free form: a doc test with `compile_fail` in `domain/verification.rs`, or `crates/horch-core/tests/verifier_compile_fail.rs` if the workspace has `trybuild`).
- `task_check_runs_as_a_job_and_the_pass_does_not_wait`.
- F2: `not_applicable` only from facts (no commits; `**Check:** none`). F3: a task with no check and no run default check fails with `no check` unless its ticket says `**Check:** none`. F5: a verdict stands only for the tip it judged; new commits make it stale.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The check command is the one today's `finish` reads (`check_command`, `crates/horch-core/src/taskledger/finish/land.rs:202`). Keep F4: the check job has its time limit (1200 s, ticket 03) and its process group is killed at the limit (Phase 1 job stop).
- `verdict.json` at `<fleet root>/tasks/<task id>/verdict.json`: 1 file for each verdict, a new name for each new verdict (`verdict-<n>.json`, protocol rule 2), written by fleetd.
- A claim attempt is not touched: its old finish runs as today (release row).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/verifier.rs`: `verifying_task_queues_1_task_check_job`, `task_check_runs_as_a_job_and_the_pass_does_not_wait`, `passed_check_of_a_code_kind_starts_the_review_steps`, `failed_check_ends_the_attempt_rejected_and_burns_1_attempt`, `same_tip_and_check_reuse_the_passed_verdict`, `new_commits_make_the_verdict_stale`, `no_check_and_no_default_fails_with_no_check`, `claim_attempt_is_not_verified_by_fleetd`; the compile-fail test.
2. Write `domain/verification.rs` (the verdict type with a private constructor), `application/verifier.rs` and `adapters/verdict_file.rs`.
3. This ticket moves a ref, lands, pushes, deletes a branch or a ref, or changes who runs the finish steps. It has a crash test: a fault point before the commit of the intended row, and after the commit and before the effect. In each case the next pass completes or fails the effect 1 time, and no tip and no work is lost.
4. Run the Check. Commit the exact files.

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

Self-hosting: this phase changes `horch spawn`, the claim and the finish steps. The build fleet itself uses the installed `horch` for these. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).

## Done when

The Check passes. A task leaves `verifying` for `done` only through the verifier, after its check job passed.
