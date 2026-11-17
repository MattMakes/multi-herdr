# 02: The next schema version: the event words of the health of a profile and the outcome context_limit

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test taskledger_schema_p7 --test taskledger_schema_v8 --test taskledger_queue --test trf_48_ledger_guard && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/src/taskledger/model.rs`, `crates/horch-core/tests/taskledger_schema_p7.rs`, `crates/horch-core/tests/taskledger_schema_v8.rs`, `crates/horch-core/tests/taskledger_queue.rs`, `crates/horch-core/tests/trf_48_ledger_guard.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode"; "Worker profiles and adapters" (The health of a profile); FM-20; "What a release does to a run in flight", row Phase 7).

## Goal

The ledger accepts the 2 event words of the health of a profile and the attempt outcome `context_limit`. No row changes.

## Requirements

- FM-19 (part): the event words `profile_unhealthy` and `profile_healthy`.
- FM-20 (part): the attempt outcome `context_limit` (MAP.md Q2).
- The release row of Phase 7: "Ledger: None." This step changes only the words that a CHECK allows. It changes no row.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Do not trust a schema number in a ticket. Read `VERSION` in `crates/horch-core/src/taskledger/schema.rs`. Add the next schema version, and keep the number in 1 constant (`PHASE_7_VERSION`). Do not write the number in other places.
- The event words are the `EventKind` of `crates/horch-core/src/taskledger/model.rs` (`HoldStarted`, `HoldEnded` and `Transition` are examples). The CHECK of `events.kind` comes from `EventKind::check("kind")`, so a new word needs a rebuild of the CHECK, as Phase 1 did (`CREATE TABLE events_v8` in `schema.rs`).
- The outcome column is `attempts.outcome`. If a CHECK lists its words, add `context_limit`. If no CHECK lists them, add the word only to the enum of outcomes, and say so in the commit message.
- The tests `taskledger_schema_v8.rs`, `taskledger_queue.rs` and `trf_48_ledger_guard.rs` pin the version and undo each step by hand. Update them for the new step.
- No other ticket of this map changes the schema.

## Steps

1. Write the tests first in `crates/horch-core/tests/taskledger_schema_p7.rs`: `p7_migration_accepts_the_profile_health_words`, `p7_migration_accepts_the_outcome_context_limit`, `p7_migration_changes_no_row` (a ledger of the previous version with rows in each table has the same rows after), `p7_newer_schema_is_refused`. Each fails before the step exists.
2. Add the step and the words. Update the 3 pinned tests.
3. Run the Check and the guard tests. Commit the exact files.

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

- The Check passes.
- A ledger of the previous version opens with the same rows and the new words.
