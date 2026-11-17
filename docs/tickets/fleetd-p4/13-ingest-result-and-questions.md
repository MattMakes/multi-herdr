# 13: fleetd ingests result.json and question files of each attempt with the checks of SR-6 and the ingested table

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --test result_ingest && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/ingest.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/result_reader.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/ingested_store.rs`, `crates/horch-core/tests/result_ingest.rs`

Spec: `docs/specs/fleetd/design.md`, "Workspace layout and file protocol" (Protocol rule 7 and the size table), "Ledger" (`ingested`; I1 and the result that wins over a lease expiry), "Order inside 1 pass", SR-6 (for `result.json`), FM-6, FM-23 (the part of `result.json`).

## Goal

Each pass reads the outbox of each live or just ended push attempt. A valid `result.json` moves the task `running -> verifying` (also from `stalled` in the same pass), 1 time. A valid question file moves it to `blocked_on_user` with the outcome `asked`. A file that fails a check is recorded `rejected` with its reason and is never read again.

## Requirements

- SR-6 for `result.json` (and the question file, which uses the same reader, Q10): a regular file; opened without following a link; its real path inside the outbox of its own attempt; under its size limit; the task id and attempt id of the payload equal those of the path. Each failure is a row in `ingested` with `outcome = rejected` and the check, and fleetd stays healthy (`symlinked_result_oversized_progress_and_foreign_attempt_id_are_rejected`, the parts of `result.json`).
- Idempotent ingestion: 1 path and 1 hash are ingested 1 time (`ingested.path`, `sha256`).
- Order: ingest runs before `renew_or_expire_lease`; a valid `result.json` present in the pass wins over a lease expiry in the same pass (I1).
- The outcomes `asked` and `quota` burn no attempt (Phase 2 rule, kept for a push attempt).
- `fm_result_file_alone_leaves_task_verifying`: a result file moves the task only to `verifying`; only the verifier moves it to `done`.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The open without following a link and the file identity come from `crates/horch-platform` (the concern `fs`, Phase P). Do not add OS code here.
- The scan of outboxes has a cursor and a time budget (Phase 1 ticket 18, `adapters/scans.rs`, kind `outboxes`). Use it; do not walk all folders in 1 pass.
- Question files: `outbox/questions/<n>.json` (ticket 16 writes them). The question row of Phase 2 (`questions` table, ticket 12 of fleetd-p2) is written by the QUESTION path; ingest moves the task and records the file.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/result_ingest.rs`: `fm_result_file_alone_leaves_task_verifying`, `symlinked_result_is_rejected_and_not_read_again`, `oversized_result_is_rejected`, `result_with_the_attempt_id_of_another_attempt_is_rejected`, `result_outside_its_outbox_is_rejected`, `same_result_is_ingested_once`, `result_wins_over_a_lease_expiry_in_the_same_pass`, `question_file_moves_the_task_to_blocked_on_user_and_burns_no_attempt`, `tmp_file_is_not_read`.
2. Write the reader, the store and the ingest function that the pass calls (ticket 46 wires it).
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

The Check passes. A valid `result.json` moves its task to `verifying` 1 time, and a bad file is rejected 1 time and never read again.
