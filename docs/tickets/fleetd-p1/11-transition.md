# 11: transition(): the 1 door, its table of allowed transitions and its events

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test arch_scan`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/ledger.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`, `crates/horch-core/src/fleetd/ideas/adapters/sqlite.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (Three rules that make the ledger trustworthy: rule 1 "One door"; "Before Phase 5, more than 1 process writes"; "A state name is never written without its entity"), OR-2, "Migration plan" ("Who writes the ledger before Phase 5").

## Goal

1 function `transition(entity, id, to_state, cause)` writes each state change of the Phase 1 entities. It looks up the table of allowed transitions, writes the row and 1 event with the cause in 1 transaction, and refuses an illegal move. A refusal writes nothing to the row, records an event, and puts 1 item on the needs-you list. The caller goes on.

## Requirements

- **OR-2** (part): every state change is an appended event with a cause. Ticket 13 adds the audit rule that finds a status write outside the door.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The entities of Phase 1 and their states (MAP.md, Q5): task (`TaskStatus` of `crates/horch-core/src/taskledger/model.rs`: `queued`, `running`, `landing`, `done`, `cancelled`), worker (`WorkerStatus`), attempt (`launching`, `live`, `ended`), job (`queued`, `running`, `passed`, `failed`, `abandoned`) and needs-you item (`open`, `resolved`, `superseded`). The task table of 12 states replaces the task part in Phase 2. Runs and worktrees keep their writes of today until the phases that give their state machines.
- Build the allowed exits of task and worker from the code of today: read each status write in `crates/horch-core/src/taskledger/` (`tasks.rs`, `workers.rs`, `finish/mod.rs`, `stop.rs`, `orchestrator.rs`, `supervisor.rs`) and in `crates/horch/src/cmd/` (`messaging.rs`, `taskledger.rs`). Each exit that today's code makes is allowed. Write the table as data in `domain/ledger.rs`, with 1 comment for each exit that names the site of today. A move of today that is not in your table is a fault of the table.
- The event: `events` has the v8 columns `entity`, `entity_id`, `from_state`, `to_state`, `cause` and `data_json` (ticket 03). Fill `kind = 'transition'` and the old columns `run_id`, `task_id`, `worker_id`, `actor` and `text` as the helper `event` of `crates/horch-core/src/taskledger/util.rs` does, so `horch ledger show` and the views of today keep working.
- The generation: "Each write transaction first checks that the generation in the `fleetd` row is its own", and "Only fleetd checks the generation". Give `transition()` an input `guard: Option<Generation>`. With `Some(g)`, the transaction first reads `fleetd.generation` and refuses with a typed error `GenerationLost` when it is not `g`. Ticket 14 gives the value and exits fleetd on that error. The `horch` commands pass `None`.
- An illegal move: no change of the row, 1 event with `to_state` = the refused state and `cause` = `illegal_transition: <cause>`, 1 needs-you item with the key `illegal_transition:<entity>:<id>`, and the error `Refused` to the caller. Each other row of the caller's work goes on (the design: "An illegal transition never stops the pass").
- The transaction: `transition()` runs inside a transaction that the caller opens (`&rusqlite::Transaction`), so a caller can write several rows and their events in 1 commit. It does no effect outside the ledger.
- The flag for the launcher: give the public function `in_door_transaction() -> bool` in `adapters/sqlite.rs`. It is true on a thread while a transaction that calls `transition()` is open. Ticket 16 refuses a process start while it is true ("No effect outside the ledger occurs inside a transaction"). Add the test `in_door_transaction_is_true_only_inside_the_transaction`.
- Ticket 10 wrote the needs-you items with the event helper of today. Move its state writes to `transition()` (`fleetd/ideas/adapters/sqlite.rs` is in `Own`).

## Steps

1. Write the tests first, in `domain/ledger.rs` and `adapters/sqlite.rs`. Each fails before the code exists:
   - `allowed_transitions_of_task_equal_the_exits_of_today` (the table against a list in the test).
   - `illegal_transition_is_refused_and_writes_nothing` (the row keeps its state; from `ddd/08-code/implementation-plan.md`, Slice 1).
   - `illegal_transition_does_not_stop_the_pass` (2 moves in 1 transaction: the illegal one is refused, the legal one commits).
   - `illegal_transition_puts_1_needs_you_item_and_1_event`.
   - `each_transition_writes_1_event_with_its_cause_and_entity`.
   - `transition_with_a_lost_generation_writes_nothing`.
2. Write the code. Move the writes of `fleetd/ideas/adapters/sqlite.rs` to `transition()`.
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

The Check passes. A legal move writes the row and 1 event in 1 transaction. An illegal move writes no row change, 1 event and 1 needs-you item.
