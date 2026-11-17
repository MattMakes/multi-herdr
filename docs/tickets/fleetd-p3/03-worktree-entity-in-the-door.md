# 03: The worktree is an entity of the 1 door: its state table in transition(), and the rows store of worktree_custody

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/sqlite.rs`, `crates/horch-core/tests/arch_scan.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (rule 1 "One door"; "A state name is never written without its entity"; the `worktrees` table), "Worktrees: leased, reconciled, never lost" (the state table: each "Leaves when" cell is an exit).

## Goal

Each change of `worktrees.state` goes through `transition()`, with its event and its cause, and the door refuses a move that the state table of the design does not allow. `worktree_custody` has 1 store of its rows that later tickets use.

## Requirements

- **FR-11** (the record): the state of a worktree changes only through the 1 door, so each change has 1 event.
- OR-2 (kept from Phase 1): `fitness_status_writes_go_through_transition` also covers `worktrees.state`.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q3, Q10 and Q18 apply here).
- The door of Phase 1: `Entity` and `EXITS` in `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs`; the table, key and column of each entity, `state_of`, `set_state` and the event writer in `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`; `transition()` and `created()` in `fleet_runtime/application/ledger.rs` (not in `Own`: call it, do not change it).
- Add `Entity::Worktree` -> (`worktrees`, `worktree_id`, `state`). Its run and task for the event come from the row (`task_id`, and the `run_id` of that task; a scratch row gets its run from `worktree_modes.run_id`).
- The exits of `worktree` (the design's table, with the Phase 3 choices of Q10):
  - `reserved` -> `created`; `created` -> `active`; `active` -> `created`, `landing`, `orphaned`; `landing` -> `landed`, `rescued` (Phase 4 uses the second exit for an abandoned branch); `landed` -> `removed`; `orphaned` -> `rescued`; `rescued` -> `active`, `removed`.
  - A row is created through `created()` in `reserved` (a tree that fleetd makes) or in `orphaned` (a folder that the reconciler adopts, ticket 09).
  - `reserved` has 1 more exit, the drop of the row (Q18): the door deletes a `reserved` row and writes its event with the cause (for example `worktree_add_failed`). It is the only delete of the door. Add it as a function of the door, for example `drop_reserved(tx, id, cause)`, that refuses each other state.
  - `removed` is terminal.
- `status` keeps today's words and today's direct writes (`set_worktree` in `crates/horch-core/src/taskledger/finish/mod.rs`, and others). The fitness rule covers only `state`. Extend `writes_status` and `fitness_status_writes_go_through_transition` in `crates/horch-core/tests/arch_scan.rs` to an `UPDATE worktrees SET ... state =` write, and add a case to `status_write_matcher_finds_a_direct_update`. Each production write of `worktrees.state` outside the door then fails the rule.
- The store, in `crates/horch-core/src/fleetd/worktree_custody/adapters/sqlite.rs`: a `WorktreeRow` of the custody (the columns of ticket 02, `state` as the enum of this ticket) and these functions, each with 1 SQL text: `get(conn, id)`, `of_task(conn, task_id)` (the row that is not `removed`), `by_path(conn, path)`, `in_states(conn, &[state])`, `insert_reserved(tx, new)` (through `created()`), `insert_orphan(tx, new)` (through `created()`), `set_facts(tx, id, head_sha, dirty, bytes, seen_at)` (no state change, so no event), `set_owner(tx, id, attempt)`, `set_rescue_ref(tx, id, ref)`. A state change is not a function of the store: the use cases call `transition()`.
- What you read keeps today's words: no output of `horch` changes in this ticket.

## Steps

1. Write the tests first. Each fails before the code exists:
   - In `fleet_runtime/adapters/sqlite.rs` (the module tests of the door): `worktree_moves_along_each_exit_of_the_design`, `worktree_move_outside_the_table_is_refused_and_recorded`, `removed_worktree_is_terminal`, `drop_reserved_deletes_only_a_reserved_row_and_writes_its_event`.
   - In `worktree_custody/adapters/sqlite.rs`: `store_reads_back_each_column_it_writes`, `of_task_skips_a_removed_row`.
   - In `arch_scan.rs`: the new matcher case.
2. Write the code. Run the Check. Commit.

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

The Check passes. `transition(Entity::Worktree, ...)` moves a row along each exit of the design and refuses each other move, and a direct write of `worktrees.state` in production code fails `fitness_status_writes_go_through_transition`.
