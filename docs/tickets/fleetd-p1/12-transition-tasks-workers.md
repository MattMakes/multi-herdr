# 12: Route the status writes of tasks.rs and workers.rs through transition()

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib taskledger && cargo test -p horch-core --lib fleetd && cargo test -p horch-e2e --test taskledger`
**Own:** `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/workers.rs`, `crates/horch-core/src/taskledger/util.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (rule 1 "One door": "There is no other `UPDATE`"; "Before Phase 5, more than 1 process writes"), OR-2.

## Goal

Each change of `tasks.status` and `workers.status` in `crates/horch-core/src/taskledger/tasks.rs` and `workers.rs` goes through `transition()` of ticket 11, with a cause. The behavior of each `horch ledger` command stays the same.

## Requirements

- **OR-2** (part): the state changes of tasks and workers in these 2 files are events with a cause.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- `tasks.rs` has about 14 status writes and `workers.rs` about 5 (`grep -n "UPDATE tasks\|UPDATE workers" crates/horch-core/src/taskledger/tasks.rs crates/horch-core/src/taskledger/workers.rs`). A write in a `#[cfg(test)]` module stays: a test may set up a row directly.
- A write that changes the status and other columns becomes 2 writes in 1 transaction: `transition()` for the status, and an `UPDATE` of the other columns with no `status`. Keep the order of today inside the transaction.
- The cause names the command or the rule of today, for example `claim`, `requeue`, `requeue_by_operator`, `cancel`, `finish_done`.
- A write that today's code makes and that the table of ticket 11 refuses is a fault of the table. Send QUESTION; do not edit `domain/ledger.rs` (not in `Own`).
- Keep the events that the code of today writes with the helper `event` of `util.rs` only where they are not a status change. A status change gets its event from `transition()` only, so no change has 2 events. Remove a helper of `util.rs` that this ticket leaves unused.
- The e2e tests of today in `crates/horch-e2e/tests/taskledger.rs` prove that the behavior stays.

## Steps

1. Write the test first, in `tasks.rs`: `each_status_change_of_claim_requeue_and_cancel_has_1_transition_event`. It fails before the change because the events of today have no `entity` and `cause`.
2. Route each production status write of the 2 files through `transition()`.
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

The Check passes. No production `UPDATE` of `tasks.status` or `workers.status` is left in `tasks.rs` or `workers.rs`.
