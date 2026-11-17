# 46: The e2e tests of a worker whose sandbox makes the ledger read-only, and of FM-17 for Codex

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo build -p horch-e2e --bins && cargo test -p horch-e2e --test p5_read_only_worker && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p5_read_only_worker.rs`, `crates/horch-e2e/src/bin/fake-codex.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

A worker like `codex-reviewer-light`, whose sandbox makes the ledger read-only, reports progress and done with no error, and a Codex worker writes its result in its task folder.

## Requirements

- FM-17 (the task folder is in `writable_roots`; the conformance test proves the write; the old escape is not needed)
- REQUIRED item 5 of the ticket of this map
- RR-5

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The e2e harness is `crates/horch-e2e` (the fake agents of `src/bin/`, the test fleetd of `src/fleetd.rs`, the fault points). Each test uses temporary `HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB` and `FLEET_ROOT`, and sets `HORCH_TEST_ASSERT_READ_ONLY=1` for each CLI process (ticket 42).
- A test waits for an event (a row, a file, an exit), never for a time.
- Today `codex-reviewer-light` gets "attempt to write a readonly database" on `horch ledger done` and `horch ledger progress`. The test makes the ledger file and its folder read-only for the worker process (mode 0444 and 0555 in the test, or the platform sandbox of ticket 06 with no write to the ledger folder).
- `fake-codex` (`crates/horch-e2e/src/bin/fake-codex.rs`, ticket 48 of fleetd-p4) writes its result through `horch result` in the task folder that its `writable_roots` names, and fails when the folder is not writable, as the real Codex sandbox does.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests: `worker_with_a_read_only_ledger_reports_progress_and_done_with_no_error`, `fm17_codex_worker_writes_its_result_in_the_task_folder`, `fm17_codex_worker_needs_no_ledger_allow_rule`.
2. Change `fake-codex` where the test needs it. Run the tests and correct small faults inside the files of this phase.

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
