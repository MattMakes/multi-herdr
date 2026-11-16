# 10: The needs-you list: 1 item for each key, with a state

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::ideas && cargo test -p horch-core --lib taskledger::status && cargo test -p horch-core --test arch_scan`
**Own:** `crates/horch-core/src/fleetd/ideas/domain/needs_you.rs`, `crates/horch-core/src/fleetd/ideas/application/needs_you.rs`, `crates/horch-core/src/fleetd/ideas/adapters/sqlite.rs`, `crates/horch-core/src/taskledger/status.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (table `needs_you`), FM-44, "Architecture and roles" (Module owners: "The needs-you list belongs to ideas"), "Migration plan" ("The numbers of the design": retention 30 days).

## Goal

Each producer writes a needs-you item by its stable key. A second write of 1 key changes the 1 item and adds none. Only `open` items are shown. An item that is not open is removed 30 days after it ended. `horch ledger status` shows the open items.

## Requirements

- **FM-44**: each item has a stable key and a state; a producer writes by key; only `open` items are shown; an item that is not open is removed after 30 days (`retention_days`).

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The table `needs_you` comes from ticket 03 (schema v8).
- Producers in Phase 1: an illegal transition (ticket 11), a job at `abandoned_run_limit` (ticket 17), and the reap path (ticket 20). The key is `<kind>:<entity id>`, for example `illegal_transition:T-3` or `job_abandoned:<job key>`.
- The use cases: `open_item(key, kind, entity, entity_id, text)` (an `open` item of that key gets the new text; a `resolved` or `superseded` item of that key opens again), `resolve(key)`, `supersede(key)`, `prune(now, retention_days)`, `open_items()`. Each state change of an item goes through `transition()` of ticket 11 later; in this ticket, write the row and 1 event in 1 transaction with the helper of today (`crates/horch-core/src/taskledger/util.rs`, `event`). Ticket 11 moves it to `transition()`.
- `domain/` holds the rules and no SQL. `adapters/sqlite.rs` holds the SQL. `fitness_no_context_touches_foreign_tables` (ticket 01) allows `ideas` only the table `needs_you`.
- `horch ledger status` is built in `crates/horch-core/src/taskledger/status.rs`. Add a section of the open items, and the count in the JSON output.

## Steps

1. Write the tests first. Each fails before the code exists:
   - `needs_you_item_written_twice_by_key_is_1_item_and_resolves_with_its_row` (from `ddd/08-code/implementation-plan.md`, Slice 1).
   - `only_open_items_are_shown`.
   - `item_not_open_is_removed_30_days_after_it_ended` (pass the time; no wait).
   - `resolved_item_opens_again_on_a_new_write_of_its_key`.
   - In `status.rs`: `status_shows_each_open_needs_you_item`.
2. Write the code. Run the Check.

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

The Check passes. 2 writes of 1 key give 1 item, and `horch ledger status` shows only open items.
