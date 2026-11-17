# 33: A job process writes its result file; fleetd ingests it and ignores a result of another run

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_job_result_file && cargo test -p horch-core --test fleetd_jobs && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/job_result_file.rs`, `crates/horch-core/tests/fleetd_job_result_file.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

A job process (`horch job run`) writes no ledger row. fleetd reads its result file and ends the job.

## Requirements

- RR-5
- RR-8
- SR-6 (the checks at ingest)
- Open question Q14 (an answer of the orchestrator)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- Today `run_job` in `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` (line 564) waits for its row, runs the executor, and writes `result_json` and the end of the job in its own transaction.
- After this ticket (answer of the orchestrator): the job process writes `<fleet root>/run/jobs/<job id>-<run>.json` atomically (`name.tmp`, fsync, rename, mode 0600), with the job id, the run number, the outcome and the text. It reads its row read-only. The job process is a child of fleetd through the launcher, so the file is fleetd-owned; fleetd still reads it with the checks of ticket 07 (root `run/jobs/`, limit `job_result_max_bytes`). A result whose run is not the current run of the row is ignored and logged with 1 event. The pass ends the job through `transition()` and archives the file after the commit.
- A task check runs in the worker sandbox (ticket 18 of fleetd-p4); the check process cannot write `run/` (ticket 35 and following deny it), and the job process, which writes the file, is outside that sandbox.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_job_result_file.rs`: `job_process_writes_its_result_file_and_no_row`, `pass_ends_the_job_from_its_file_once`, `result_of_an_old_run_is_ignored_and_logged`, `symlinked_result_file_is_rejected`, `result_file_over_its_limit_is_rejected`, `crash_after_the_commit_only_archives_the_file`.
2. Change `run_job`, write the reader, and keep `crates/horch-core/tests/fleetd_jobs.rs` green (the pass reads the file; ticket 41 wires the call).

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
- The 6 tests and `fleetd_jobs` pass.
