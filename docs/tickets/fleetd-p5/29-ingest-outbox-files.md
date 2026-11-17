# 29: fleetd ingests progress.jsonl, the handoff result and the owner and mode checks of each outbox file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test outbox_ingest && cargo test -p horch-core --test result_ingest && cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/outbox_reader.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/result_reader.rs`, `crates/horch-core/tests/outbox_ingest.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

fleetd turns each valid outbox file into ledger rows 1 time, and records each invalid file as rejected with its reason.

## Requirements

- SR-6 (each worker file)
- RR-8 (ingestion is idempotent; a log is read only to its last complete line)
- FM-23
- pres 04 M5 (a foreign-owned outbox is refused)
- Open question Q3 and Q4

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- `result.json` and question files are ingested since Phase 4 (ticket 13 of fleetd-p4: `application/ingest.rs`, `adapters/result_reader.rs`, `adapters/ingested_store.rs`, test `crates/horch-core/tests/result_ingest.rs`). This ticket moves `result_reader.rs` to the protected file reader of ticket 07, so each outbox file also passes the owner and mode checks, and the outbox folder that a worker command made (Q3) passes the same checks (folder 0700, owner).
- `progress.jsonl`: read from the offset in `ingested` (`log_offset`), only to the last complete line, at most `progress_max_bytes` and `progress_max_lines`, each line at most `progress_line_max_bytes` (ticket 03). Each line with the task id and attempt id of its path becomes the row that `horch ledger progress` writes today (the progress record and the activity of the attempt, Phase 2 Q7), through `transition()` where a state moves. A line that fails is skipped with 1 event; the file is rejected only when it is not a regular file, is a link, has a wrong owner or mode, or is over its byte limit.
- The handoff result (Q4): `status: blocked` and `reason: handoff`. The attempt ends with the outcome `handed_off` (the word of today for a handed-off worker) and burns no attempt. The event says `handoff`. The handoff path is an untrusted worker path: fleetd keeps it only when its real path is inside the worktree or the task folder of the attempt, and `START.md` of the next attempt names it (the writer of `START.md` is ticket 07 of fleetd-p4, `adapters/task_files.rs`: this ticket gives it the path as an input field; if that needs a change of that file, send QUESTION).
- fleetd scans only the outboxes of live attempts and of attempts that ended since the last pass, with the scan cursor `outboxes` (Phase 1 scans).
- `idempotent:` tests of each file-borne message (DDD Slice 5): the same file read 2 times has 1 effect.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/outbox_ingest.rs`: `progress_lines_are_ingested_once_from_the_recorded_offset`, `half_written_last_line_waits_for_the_next_pass`, `progress_over_its_limit_is_rejected_and_fleetd_stays_healthy`, `line_with_the_attempt_id_of_another_attempt_is_skipped`, `pres_04_m5_foreign_owned_outbox_is_refused`, `outbox_with_mode_0755_is_refused`, `symlinked_result_is_rejected`, `handoff_result_ends_the_attempt_handed_off_and_burns_no_attempt`, `handoff_path_outside_the_attempt_is_not_used`, `idempotent_progress_line`, `idempotent_result_file`, `idempotent_question_file`.
2. Move `result_reader.rs` to ticket 07, write the progress reader and the ingest of the handoff. Keep `result_ingest.rs` green.

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
- The 12 tests and the tests of `result_ingest.rs` pass.
- Each rejected path is in `ingested` with its reason and is not read again.
