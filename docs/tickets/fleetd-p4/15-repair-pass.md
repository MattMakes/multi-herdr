# 15: The repair pass: an agent that exits with commits and no result gets 1 short resume of its session to write the result

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch --bin horch -- worker && cargo test -p horch-core --lib fleetd::task_execution::domain::repair && cargo test -p horch-core --test repair_pass && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/worker.rs`, `crates/horch-core/src/fleetd/task_execution/domain/repair.rs`, `crates/horch-core/tests/repair_pass.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" (Finishing without reporting is closed at both ends), "Worker profiles and adapters" (`result_gate`: the wrapper's repair pass, which also accepts a question file), FM-6.

## Goal

When the agent process of a push attempt exits with commits since its base and with no `result.json` and no question file, `horch worker` runs 1 short resume of the same session with the fixed line "Write the result of your task now: run `horch result`.", then exits. It never runs a second repair of 1 attempt.

## Requirements

- FM-6 (the outside end): worst case 1 retry that starts from the work; the next attempt reuses the same worktree (Phase 3 ticket 11).
- The repair burns no attempt. After the repair, no result gives the outcome `no_result` (rung 6).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Until Phase 5 there is no fleet-run wrapper: `horch worker` (`crates/horch/src/cmd/worker.rs`, the supervisor of `crates/horch-core/src/taskledger/supervisor.rs`) is the wrapper (Phase 2 Q6). Its agent exit handling is the place of the repair.
- The resume is the native session resume of the CLI (`Session::Resume` of the harness), started through the launcher. It is not the native agent resume of herdr (SR-7).
- The decision (pure) is `domain/repair.rs`: `fn needs_repair(facts) -> bool` (exited, commits since base, no result, no question file, no earlier repair). The record of the repair is 1 event on the attempt.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `exit_with_commits_and_no_result_runs_1_repair`, `exit_with_a_result_runs_no_repair`, `exit_with_a_question_file_runs_no_repair`, `exit_with_no_commits_runs_no_repair`, `second_repair_of_1_attempt_never_runs`, `repair_burns_no_attempt`.
2. Write the decision and the resume in `worker.rs`.
3. Run the Check. Commit the exact files.

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

The Check passes. An agent that exits with commits and no result gets 1 repair resume, and then the next attempt starts from its work.
