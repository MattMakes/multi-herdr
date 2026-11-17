# 34: The polite stop of a process is a ledger row, not a file in run/stops

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_polite_stops && cargo test -p horch-core --test fleetd_pass && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/polite_stops_store.rs`, `crates/horch-core/tests/fleetd_polite_stops.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

The time of a polite stop survives a restart of fleetd as a row of `polite_stops`, so no file in the run folder holds a fact that fleetd acts on.

## Requirements

- RR-5
- RR-8 (each effect is a committed row first)
- REQUIRED item 4 of the ticket of this map (Open question Q12)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- Today `polite_path` in `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs` (line 218) writes `<fleet root>/run/stops/<pid>-<start>.json` (task T-414), and lines 305 and 344 read the folder.
- Choice Q12: a row of `polite_stops` (ticket 02), written in the transaction that records the stop, before the signal. The kill comes when the grace ends on the fleet clock. The row is deleted when the process is gone. Why: from Phase 5 fleetd is the only writer, so the row is cheap; a row has no file to check (SR-6); the sandbox denies a write in the run folder, and the row does not depend on it.
- At the first pass after the update, fleetd reads each old file of `run/stops/` 1 time with the checks of ticket 07, writes its row, and removes the file. A file that fails a check is removed and logged, and its process gets a new grace.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_polite_stops.rs`: `polite_stop_is_a_row_before_the_signal`, `kill_comes_after_the_grace_across_a_restart`, `row_is_deleted_when_the_process_is_gone`, `old_stop_file_becomes_a_row_once`, `symlinked_old_stop_file_is_removed_and_logged`.
2. Change `effects.rs` and write the store. Keep `crates/horch-core/tests/fleetd_pass.rs` green.

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
- The 5 tests pass.
- No production code writes `run/stops/`.
