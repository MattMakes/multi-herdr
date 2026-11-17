# 42: Each process other than fleetd opens the ledger read-only, and the arch rule that enforces it

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test arch_scan && cargo test -p horch-core --test fleetd_single_writer && cargo test --workspace --locked`
**Own:** `crates/horch-core/src/taskledger/mod.rs`, `crates/horch-core/src/taskledger/store.rs`, `crates/horch-core/tests/arch_scan.rs`, `crates/horch-core/tests/fleetd_single_writer.rs`, `crates/horch/src/cmd/compact.rs`, `crates/horch/src/cmd/taskledger_wake.rs`, `crates/horch-core/src/telemetry/collect.rs`, `crates/horch-core/src/execution/records.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

Only fleetd (and `horch ledger watch --once` while it holds the fleetd lock) opens the ledger for a write, and a test fails when any other code does.

## Requirements

- RR-5 (SQLite opened read-only everywhere else)
- FM-24
- the arch test of the Phase 5 gate
- `cli_command_opens_ledger_read_only_and_sends_a_request` (DDD Slice 5)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- Today `taskledger::open` (`crates/horch-core/src/taskledger/mod.rs`, line 38) creates and migrates the ledger, and many readers call it. After the routing tickets, the writers left are fleetd and its pass. Make the writable open a capability that only the fleetd composition root (`crates/horch/src/cmd/fleetd.rs`) and the holder of the fleetd lock can make; each other caller uses `open_existing_read_only`.
- A command that finds no ledger file: fleetd creates and migrates the ledger at its start. The command does not create it; a read prints what it prints today for a missing ledger, and a change writes a request file (ticket 15).
- The arch rule (Phase 1 Q13 names it): `fitness_only_fleet_runtime_opens_the_ledger_writable` in `crates/horch-core/tests/arch_scan.rs`: no production file outside `crates/horch-core/src/fleetd/` and `crates/horch/src/cmd/fleetd.rs` calls `Store::open`, `taskledger::open` or `open_existing`. Each exception has a line in the rule with its reason (for example the schema migration code inside `store.rs`).
- The test seam `HORCH_TEST_ASSERT_READ_ONLY=1`: a writable open in a process with this variable panics. The e2e tests of tickets 43 to 47 set it for each CLI process.
- The files of `Own` besides `mod.rs`, `store.rs` and the tests are the readers that no other ticket of this map owns (found with `grep -rn "taskledger::open(\|Store::open(\|open_existing(" crates`). If the grep finds a production writer in another file, stop and send QUESTION.
- The Check runs the whole workspace, because this ticket changes the open of each caller.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: the arch rule above, and `crates/horch-core/tests/fleetd_single_writer.rs` with `cli_command_opens_ledger_read_only_and_sends_a_request`, `read_command_on_a_read_only_ledger_file_works`, `writable_open_outside_fleetd_panics_under_the_seam`.
2. Change the open functions and each remaining caller.

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
- The tests pass.
- `cargo test --workspace --locked` passes.
