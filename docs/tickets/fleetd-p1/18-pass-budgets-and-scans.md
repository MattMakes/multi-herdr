# 18: The work budget of a pass, the cursors in pass_cursors, and the fact scans with their time budget

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --test fleetd_scans`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/budget.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/cursors_store.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/scans.rs`, `crates/horch-core/tests/fleetd_scans.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" (A pass has a work budget; the fact scans; the time budget of a pass), "Ledger" (table `pass_cursors`), FM-7, FM-12, PL-5 (the cursor part).

## Goal

A pass handles at most `pass_budget` rows of each kind and continues at a cursor on the next pass. Each fact scan (processes, outboxes, worktree folders, requests) has a cursor and a time budget, stops at its budget, and the next pass continues at its cursor, also after a restart. A changed folder that file watching reports goes first.

## Requirements

- **FM-7** (part): scans with a cursor reach each item in turn instead of trusting events.
- **FM-12** (part): the fact scans and the rows of each pass have a budget with a cursor, and a restart continues each scan at its cursor.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The table `pass_cursors` comes from ticket 03; `pass_budget`, `scan_budget_ms` and `pass_time_budget_s` from ticket 04.
- The scans of Phase 1 (MAP.md, Q4): `processes` (the PIDs that rows of `workers`, `attempts` and `jobs` name, alive by PID and start time through the platform process concern); `outboxes` (the folder `<fleet root>/tasks/<task id>/attempts/<attempt id>/outbox/` of each attempt that is not `ended`: the scan reads the folder listing only, because ingestion comes in Phase 4); `worktree_folders` (the folder of each `worktrees` row that is not cleaned: exists or not); `requests` (the folder `<fleet root>/run/requests/`: empty until Phase 5; the scan exists so the cursor kind exists). Each scan goes in order of its key, from the cursor `after_key`, and wraps.
- File watching (ticket 07) is a hint: the scan puts a reported folder first, and then goes on at its cursor.
- `domain/budget.rs`: the pure rules: `PassBudget` (the count of rows of each kind), `Deadline` of a pass (`pass_time_budget_s`) and of a scan (`scan_budget_ms`), and the cursor step (after_key, wrap, `lap_started_at`, `prev_lap_started_at`). `adapters/scans.rs`: the 4 scans. `adapters/cursors_store.rs`: read and write `pass_cursors` in the transaction of the pass.
- The cursor of a scan is saved in the transaction of part 2 of the same pass (part 1 has no transaction), so a crash before the commit scans the same items again: no item is skipped.

## Steps

1. Write the tests first, in the new file `crates/horch-core/tests/fleetd_scans.rs` and in `domain/budget.rs`. Each fails before the code exists:
   - `each_scan_continues_at_its_cursor_after_a_restart` (Slice 1): 10,000 outbox folders in a temp fleet root and a small budget; 2 passes with a new process state between them read disjoint items in order.
   - `a_scan_stops_at_its_time_budget`.
   - `the_cursor_wraps_and_each_item_gets_its_turn`.
   - `a_watched_change_goes_first`.
   - `rows_past_the_pass_budget_wait_for_the_next_pass`.
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

The Check passes. 10,000 folders are scanned in turns within the budget, and a restart continues at the cursor.
