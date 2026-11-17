# 02: The next schema version: the state, owner and rescue columns of worktrees, and their backfill

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test taskledger_schema_p3 && cargo test -p horch-core --lib taskledger::schema && cargo test -p horch-core --test taskledger_schema_v8 && cargo test -p horch-core --test taskledger_schema_p2 && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/tests/taskledger_schema_p3.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (the `worktrees` table), "Worktrees: leased, reconciled, never lost" (the state table; "Built on what horch has"), "Migration plan" (item 5; "Schema migration is forward-only and runs at daemon start"; the row Phase 3 of "What a release does to a run in flight"); `docs/specs/fleetd/preservation-05-contracts-and-values.md` rows L-3, L-4 and L-5.

## Goal

The ledger has the next schema version. The existing table `worktrees` gains the columns of Phase 3. The migration fills them for each row of a run in flight, so a live worker goes on in its tree and each other tree is `orphaned` and is rescued before anything else.

## Requirements

- **FR-11** (the record): each worktree is a row with a state and an owner.
- The backfill of the row Phase 3 of "What a release does to a run in flight": each `worktrees` row whose task has a live attempt becomes `active`, with that attempt as owner. Each other row becomes `orphaned`.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q2, Q3 and Q19 apply here).
- The version: read `VERSION` in `crates/horch-core/src/taskledger/schema.rs`. This ticket adds the next number, with 1 migration step. Do not write the number in a ticket, a comment or a doc: use `VERSION` (the standing rule of the orchestrator: each schema number is in 1 constant).
- Today's table (`worktrees_table()` in `schema.rs`): `worktree_id` (key), `task_id` (null for a scratch tree), `worker_id`, `repo_root`, `path` (unique), `branch`, `base`, `base_commit`, `status` (today's words, `V1_WORKTREE_STATUS`), `landed_commit`, `bytes_on_disk`, `origin`, `created_at`, `landed_at`, `cleaned_at`, `reason`. The table `worktree_modes` holds the name and the scratch flag.
- The new columns (Q2): `state TEXT NOT NULL` with `CHECK (state IN ('reserved','created','active','landing','landed','removed','orphaned','rescued'))`; `owner_attempt TEXT REFERENCES attempts(id)` (null while no attempt holds the tree); `head_sha TEXT`; `dirty INTEGER NOT NULL DEFAULT 0`; `rescue_ref TEXT`; `seen_at TEXT`. `base_commit` is the base of the design and `bytes_on_disk` is its `disk_bytes`: do not add a second column for them. `task_id` stays nullable (scratch trees, and adopted orphans with no task). No `idea_id` before Phase 6.
- `status` keeps today's words (Q3). A new row gets both columns.
- The backfill (Q19), in the migration step, in this order:
  1. `status` `cleaned` -> `state` `removed`.
  2. A row whose task has an `attempts` row in `live` for the worker of the row (`attempts.worker_id`, Phase 1) -> `active`, `owner_attempt` = that attempt. A scratch row (`worktree_modes.scratch = 1`) whose worker has a live attempt -> `active` with that attempt.
  3. Each other row -> `orphaned`, `owner_attempt` null.
  The migration reads no process and no folder. The first pass of fleetd checks each fact again (ticket 09): a dead owner makes the row `orphaned`, and a folder that is gone is escalated.
- Today's safety stays (L-3, L-4, L-5): a newer schema is refused, a backup file is written before the migration, and a development build does not migrate the shared ledger. Do not change those paths.
- The migration tests of Phases 1 and 2 (`crates/horch-core/tests/taskledger_schema_v8.rs`, `crates/horch-core/tests/taskledger_schema_p2.rs` from ticket 02 of fleetd-p2) show how to stop the migration at an older version and write rows. Use their helper; do not change those files.
- No test opens `/Users/MASCOTT/.horch/ledger.sqlite3`. Each test makes its ledger in a temporary folder.

## Steps

1. Write the tests first, in `crates/horch-core/tests/taskledger_schema_p3.rs`. Each fails before the migration exists:
   - `worktrees_gains_state_owner_head_dirty_rescue_and_seen_columns` (the columns and the CHECK of `state`; a write of an unknown state word is refused).
   - `backfill_makes_a_tree_with_a_live_attempt_active_with_its_owner`.
   - `backfill_makes_each_other_tree_orphaned`, with 1 row of each `status` word of today.
   - `backfill_makes_a_cleaned_tree_removed`.
   - `backfill_makes_a_scratch_tree_of_a_live_worker_active`.
   - `migration_to_the_phase_3_version_writes_a_backup_first` (L-4).
   - `migration_from_phase_2_keeps_every_worktree_row_and_its_status`: the count of rows, each `status`, `path` and `branch` are the same after the migration.
2. Write the migration step and the new `worktrees_table()` text for a new ledger.
3. Run the Check. Commit.

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

Self-hosting: each test makes its own temporary git repository and temporary folders (build rule 1). No test and no worker runs the reconciler, a rescue, a cleanup or a `git worktree remove` on the repository `/Users/MASCOTT/projects/mh/multi-herdr` or on its `.worktrees/` folder.

## Done when

The Check passes. A ledger of Phase 2 migrates to the next version with each worktree row kept, each row has a state, and a row with a live attempt is `active` with that attempt as owner.
