# 14: The request files: the folder modes, the checks of each file, the requests row, then the archive

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_request_files && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`, `crates/horch-core/tests/fleetd_request_files.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

When fleetd is down, a command writes a request file. fleetd reads each request file at start and on each pass, applies it 1 time, and archives it only after its row is committed.

## Requirements

- SR-5 (the request folder 0700, each file 0600, the owner check)
- SR-6 (a request file is untrusted)
- RR-8 (a request file is archived only after its row is committed)
- FM-15
- FM-23

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The folder: `<fleet root>/run/requests/` with mode 0700, and `archive/` inside it. fleetd makes them when they are missing, and refuses to read a folder with another owner or a wider mode (1 needs-you item).
- Each file `<request_id>.json`, mode 0600. Readers skip `*.tmp`. The checks of ticket 07 with the root `run/requests/` and the limit `request_max_bytes`. A file that fails is recorded in `ingested` as rejected with its reason and is not read again. A file that parses gets its `requests` row through ticket 12, with the source "request file" and the owner as caller.
- The scan has a cursor in `pass_cursors` (kind `requests`) and the scan budget of a pass (Phase 1, ticket 18 of fleetd-p1, `crates/horch-core/src/fleetd/fleet_runtime/adapters/scans.rs`). Ticket 41 calls it from the pass.
- The crash cases of the design: a crash before the commit leaves the file, and the next pass reads it again; a crash after the commit finds the row by its `request_id`, and the file is only archived.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_request_files.rs`: `request_file_is_applied_then_archived`, `file_with_mode_0644_is_refused_with_its_reason`, `file_of_another_owner_is_refused` (owner seam), `symlink_request_file_is_rejected`, `tmp_file_is_skipped`, `crash_before_commit_reads_the_file_again_with_1_effect`, `crash_after_commit_only_archives`, `folder_with_mode_0755_is_refused_and_opens_1_needs_you_item`, `scan_continues_at_its_cursor`.
2. Write `fn scan(store, fleet_root, cursor, budget, handlers) -> ScanOutcome`.

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
- The 9 tests pass.
- No request file is archived before its row.
