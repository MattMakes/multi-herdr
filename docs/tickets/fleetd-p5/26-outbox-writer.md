# 26: The outbox writer of a worker command: find the own attempt by the process chain, make the outbox, write each file atomically

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test outbox_writer && cargo test -p horch-core --lib fleetd::task_execution::domain::outbox && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/outbox.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/outbox_writer.rs`, `crates/horch-core/tests/outbox_writer.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

A worker command writes `progress.jsonl`, `result.json` and a question file into the outbox of its own attempt, and of no other attempt, with no ledger write.

## Requirements

- FR-7 (each hand-off is a file under the fleet root)
- RR-5 (a worker writes files, fleetd ingests them)
- RR-8 (atomic writes; a log is appended by whole lines)
- SR-6
- Open question Q3 and Q4 (answers of the orchestrator)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The files and their kinds are in the design, Workspace layout and file protocol: `outbox/result.json` (immutable message), `outbox/questions/<n>.json` (immutable message), `outbox/progress.jsonl` (append-only log). The outbox is `<fleet root>/tasks/<task id>/attempts/<attempt n>/outbox/` (Open question Q3 of `docs/tickets/fleetd-p4/MAP.md`).
- `domain/outbox.rs` holds the file formats: the `result.json` of `crates/horch-core/src/fleetd/task_execution/domain/result.rs` (ticket 12 of fleetd-p4) is reused, with 1 addition for the handoff (Q4): `"status": "blocked"` with `"reason": "handoff"` and `"handoff": <absolute path>`. The status list stays `completed`, `blocked`, `failed`. A progress line: `{"task", "attempt", "at", "kind": "progress"|"note", "text"}`, at most `progress_line_max_bytes`.
- The own attempt (answer of the orchestrator to Q3): the command reads the live attempts read-only and takes the 1 attempt whose wrapper (PID and start time) is in the parent chain of the command process. It never reads an environment variable for the task, the attempt or the role. No attempt found: the command refuses with `no live attempt of this process`. So a worker cannot choose the attempt id of another attempt.
- The outbox folder is made by the command when it is missing (Q3), with mode 0700; each file is written with mode 0600. A message: `name.tmp`, fsync, rename, fsync of the folder (`horch_platform::fs::sync_dir`). A log line: 1 `write` of the whole line with `O_APPEND`. fleetd checks each of these at ingest (ticket 29), so a wrong mode or owner is caught there too.
- The same facts as ticket 11: the process facts of `crates/horch-platform/src/process.rs`.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/outbox_writer.rs`: `command_under_a_wrapper_finds_its_own_attempt`, `worker_that_sets_the_task_or_attempt_of_another_attempt_in_its_environment_writes_only_its_own_outbox`, `process_outside_each_attempt_is_refused`, `missing_outbox_is_made_with_mode_0700`, `result_is_written_atomically_with_mode_0600`, `progress_lines_from_2_processes_never_interleave`, `line_over_the_limit_is_refused_before_the_write`, `handoff_result_has_status_blocked_and_reason_handoff`.
2. Write the formats in `domain/outbox.rs` and the writer in `adapters/outbox_writer.rs`.

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

- The Check passes, with the guard tests.
- The 8 tests pass.
- The writer has no input from the environment for the attempt.
