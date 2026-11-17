# 45: The e2e tests of requests: 1 request_id has 1 effect, the crash cases, and a lost reply

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-e2e --test p5_requests && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p5_requests.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

The gate rows RR-8 and FM-15 have e2e proof with real processes.

## Requirements

- RR-8 (the Phase 0 test: a request sent 2 times with 1 `request_id` has 1 effect)
- FM-15

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The e2e harness is `crates/horch-e2e` (the fake agents of `src/bin/`, the test fleetd of `src/fleetd.rs`, the fault points). Each test uses temporary `HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB` and `FLEET_ROOT`, and sets `HORCH_TEST_ASSERT_READ_ONLY=1` for each CLI process (ticket 42).
- A test waits for an event (a row, a file, an exit), never for a time.
- The fault points of `crates/horch-e2e` kill fleetd at a named point: use them (or add 1 point in a file of this phase) for "after the row, before the commit" and "after the commit, before the archive".
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests: `request_sent_2_times_with_1_request_id_has_1_effect`, `fleetd_killed_before_the_commit_applies_the_request_file_once_after_restart`, `fleetd_killed_after_the_commit_only_archives_the_file`, `lost_reply_is_found_by_horch_request_and_not_applied_again`, `known_request_id_with_another_payload_is_refused`.
2. Run them and correct small faults inside the files of this phase.

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
- Each test passes.
