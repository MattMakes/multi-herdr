# 47: The release test of Phase 5: an old worker of Phase 4 runs progress and done after the update, and only fleetd writes

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-e2e --test p5_release && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p5_release.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

An update to Phase 5 during a run loses no report and stops no run.

## Requirements

- The row Phase 5 of "What a release does to a run in flight": "An old worker on Phase 4 runs `progress` and `done` after the update. Each one has 1 effect, and only fleetd writes the ledger."
- Open question Q3 (the outbox of a live attempt)
- FM-24

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The e2e harness is `crates/horch-e2e` (the fake agents of `src/bin/`, the test fleetd of `src/fleetd.rs`, the fault points). Each test uses temporary `HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB` and `FLEET_ROOT`, and sets `HORCH_TEST_ASSERT_READ_ONLY=1` for each CLI process (ticket 42).
- A test waits for an event (a row, a file, an exit), never for a time.
- The old worker: a live attempt of Phase 4 (a claim attempt and a push attempt, 1 test each), started before the update with the rows of Phase 4 and no outbox for the claim attempt. "The update" in the test: stop the test fleetd of Phase 4 rules, migrate to the next schema version, start the test fleetd of this phase. The worker process goes on.
- Proof that only fleetd writes: the CLI processes run with `HORCH_TEST_ASSERT_READ_ONLY=1`, and the progress and the result are in the ledger only after fleetd ingests them.
- The run is not stopped: its run row and its other attempts keep their states.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests: `old_claim_worker_runs_progress_and_done_after_the_update_with_1_effect_each`, `old_push_worker_runs_progress_and_done_after_the_update_with_1_effect_each`, `update_stops_no_run`.
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
