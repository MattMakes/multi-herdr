# 02: The next schema version: ideas, requirements, task_covers and trial_merges, the idea of each task, fix_rounds, and the release backfill

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test taskledger_schema_p6 && cargo test -p horch-core --test taskledger_schema_v8 && cargo test -p horch-core --test taskledger_queue && cargo test -p horch-core --test trf_48_ledger_guard && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch-core --lib taskledger::schema`
**Own:** `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/tests/taskledger_schema_p6.rs`, `crates/horch-core/tests/taskledger_schema_v8.rs`, `crates/horch-core/tests/taskledger_queue.rs`, `crates/horch-core/tests/trf_48_ledger_guard.rs`

Spec: `docs/specs/fleetd/design.md` (Ledger (Schema: `CREATE TABLE ideas`, `requirements`, `task_covers`, `trial_merges`, `tasks`, `attempts`); Migration plan, item 8 "Phase 6: the idea dispatcher and sub-orchestrators"; "What a release does to a run in flight", row Phase 6).

## Goal

The ledger has the idea tables of the design and the columns that tie each task and each sub-orchestrator attempt to an idea, in 1 new schema version, with a backfill that gives each run in flight 1 idea.

## Requirements

- FR-12 (each task belongs to an idea)
- RR-3 (`fix_rounds` of a judged task)
- The release row of Phase 6: each run gets 1 idea row

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Schema: do not write a schema number. Use the next schema version: read `VERSION` in `crates/horch-core/src/taskledger/schema.rs` and add 1, as a new constant `PHASE_6_VERSION` beside `PHASE_2_VERSION` and the constants of Phases 3 to 5. Keep the number in that 1 constant. `VERSION` points at it.
- This ticket owns the tests that pin the version and undo each step by hand: `crates/horch-core/tests/taskledger_schema_v8.rs`, `taskledger_queue.rs` and `trf_48_ledger_guard.rs`. Make them follow the new step.
- Copy the DDL of `ideas`, `requirements`, `task_covers` and `trial_merges` from the design exactly. `ideas.feature_id` references `features(id)` (Phase 4, ticket 02 of fleetd-p4).
- `tasks`: add `idea_id TEXT REFERENCES ideas(id)` and `fix_rounds INTEGER NOT NULL DEFAULT 0` (no earlier phase adds it: check `schema.rs`). SQLite cannot add a NOT NULL column with no default, so add the trigger `tasks_need_an_idea`: an INSERT of a task with a null `idea_id` aborts with the text `a task needs an idea (FR-12)`.
- `attempts`: add `idea_id TEXT REFERENCES ideas(id)`. A sub-orchestrator attempt has a null `task_id` and an `idea_id`.
- Backfill (MAP.md, Q5): each run with a `features` row gets 1 idea row `I-<run seq>` (4 digits, zero-padded) in that feature. Its state is `in_progress` when a task of the run is not `done` and not `cancelled`, else `integrating`. Each task of the run gets that `idea_id`. `approved_sha256` holds the marker `release` (MAP.md, Q6). Each `questions` row gets `feature_id` = the feature of its run, and `owner` = `idea-lead:<feature id>` (MAP.md, Q11). A run with no feature row (no run branch) gets an idea in its own new feature row only when it has a task that is not `done` and not `cancelled`; else it gets none.
- The migration reads no file and starts no process. Ticket 50 writes the idea folders at the first pass.
- Today's safety stays (`preservation-05-contracts-and-values.md`, rows L-3, L-4, L-5): a newer schema is refused, a backup file is written before each migration, a development build does not migrate the shared ledger.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/taskledger_schema_p6.rs`: `ideas_table_has_the_10_states_of_the_design` (a row in state `done` is refused by the CHECK), `task_without_an_idea_is_refused_by_the_trigger`, `backfill_gives_each_run_1_idea_in_its_feature`, `backfill_idea_is_integrating_when_each_task_is_done_or_cancelled`, `backfill_moves_each_question_to_the_feature_and_its_sub_orchestrator`, `trial_merges_key_is_the_2_tips`, `migration_to_the_next_version_writes_a_backup_first`, `newer_schema_is_still_refused`. They fail because the tables do not exist.
2. Add the migration step and the constant. Raise `VERSION`.
3. Add the new tables to each list of tables in `schema.rs` that the schema tests read. Make the 3 pinning tests follow the new step.
4. Run the Check.
5. Run the Check and the guard tests. Commit the exact files.

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
- A ledger of the earlier version with 2 runs and 1 open question migrates with a backup, and each task has an idea.
