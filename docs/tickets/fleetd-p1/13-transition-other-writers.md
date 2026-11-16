# 13: Route the other status writes through transition(), and the audit rule of OR-2

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib taskledger && cargo test -p horch-core --test arch_scan && cargo test -p horch && cargo test -p horch-e2e --test taskledger --test lifecycle`
**Own:** `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/taskledger/stop.rs`, `crates/horch-core/src/taskledger/orchestrator.rs`, `crates/horch-core/src/taskledger/supervisor.rs`, `crates/horch/src/cmd/messaging.rs`, `crates/horch/src/cmd/recipes.rs`, `crates/horch/src/cmd/sessionscmd.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch/src/cmd/taskledger_spawn.rs`, `crates/horch-core/tests/arch_scan.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (rule 1 "One door"), OR-2 ("Event table audit"), "Migration plan" ("Who writes the ledger before Phase 5").

## Goal

Each other production write of `tasks.status` and `workers.status` goes through `transition()`. An `arch_scan` rule finds a status write outside the door, so OR-2 stays true.

## Requirements

- **OR-2**: every state change is an appended event with a cause. The audit is the rule `fitness_status_writes_go_through_transition` and the test `every_status_change_has_an_event_with_a_cause`.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The sites (`grep -rn "UPDATE tasks\|UPDATE workers" crates/horch-core/src crates/horch/src`), production only: `taskledger/finish/mod.rs` (`landing`, the end of a task and of its worker), `taskledger/orchestrator.rs` (the status of the orchestrator worker), `taskledger/supervisor.rs:366`, `taskledger/stop.rs`, `crates/horch/src/cmd/messaging.rs:1395`, and each site of `recipes.rs`, `sessionscmd.rs`, `taskledger.rs` and `taskledger_spawn.rs` that is not in a test module. Check each one: many are in `#[cfg(test)]` modules, and those stay.
- `taskledger/watchdog.rs` and `crates/horch/src/cmd/taskledger_watch.rs` are not in `Own`: tickets 22 and 25 move them into fleetd. The rule lists these 2 files as exceptions by path, each with the ticket that removes the exception.
- The rule `fitness_status_writes_go_through_transition` in `crates/horch-core/tests/arch_scan.rs`: no production line of `horch-core` or `horch` holds `UPDATE tasks` or `UPDATE workers` with `status` in the same statement, outside `fleetd/fleet_runtime/adapters/sqlite.rs` and the 2 exceptions. Skip test code as the other rules of the file do.
- The rule of ticket 12 applies: a status write and its other columns are 2 writes in 1 transaction, and the cause names the command.

## Steps

1. Write the tests first:
   - `fitness_status_writes_go_through_transition` and its sample test `status_write_matcher_finds_a_direct_update`. The rule fails before the change, on the sites above.
   - In `finish/mod.rs`: `every_status_change_has_an_event_with_a_cause` (a task through claim, finish and done; each status change has 1 event with `entity`, `from_state`, `to_state` and `cause`).
2. Route each site through `transition()`.
3. Run the Check.

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

The Check passes. The rule finds no direct status write outside the door and its 2 named exceptions.
