# 02: The next schema version: the requests table with class and auth, and the polite_stops table

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test taskledger_schema_p5 && cargo test -p horch-core --lib taskledger::schema && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/tests/taskledger_schema_p5.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

The ledger has the `requests` table of the design and the `polite_stops` table, in 1 new schema version, with the backup and the refusals of today.

## Requirements

- RR-8 (the `requests` row of each command request)
- SR-8 (the `class` and `auth` columns)
- Open question Q12 (the polite stop is a row)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- Schema: do not write a schema number. Use the next schema version: read `VERSION` in `crates/horch-core/src/taskledger/schema.rs` and add 1. Keep the number in that 1 constant.
- The DDL of `requests` is in the design, section Ledger (`CREATE TABLE requests`): `request_id` (key), `kind`, `payload_json`, `payload_sha256`, `caller`, `class` with `CHECK (class IN ('operator','worker'))`, `auth`, `state` with `CHECK (state IN ('pending','applied','refused'))`, `result_json`, `reason`, `received_at`, `decided_at`. Copy it exactly.
- `polite_stops` (Open question Q12): `pid INTEGER NOT NULL`, `pid_start TEXT NOT NULL`, `at_s INTEGER NOT NULL` (the fleet clock), `pane TEXT`, primary key (`pid`, `pid_start`). It replaces the file `<fleet root>/run/stops/<pid>-<start>.json` of `polite_path` in `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs` (task T-414). Ticket 34 moves the writes.
- The release backfill of Phase 5 is empty: the `requests` table starts empty (design, "What a release does to a run in flight"). The migration reads no file. Ticket 34 reads the old files of `run/stops/` 1 time, at the first pass.
- Today's safety stays (`preservation-05-contracts-and-values.md`, rows L-3, L-4, L-5): a newer schema is refused, a backup file is written before each migration, a development build does not migrate the shared ledger.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/taskledger_schema_p5.rs`: `requests_table_has_the_columns_and_checks_of_the_design` (a row with class `admin` or state `done` is refused by the CHECK), `polite_stops_key_is_pid_and_start`, `migration_to_the_next_version_writes_a_backup_first`, `newer_schema_is_still_refused`. They fail because the tables do not exist.
2. Add the migration step and raise `VERSION` by 1.
3. Add `requests` and `polite_stops` to each list of tables in `schema.rs` that the tests of the schema read.

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
- The 4 tests pass.
- A ledger of the earlier version migrates with a backup, and its rows are unchanged.
