# 17: fleetd handlers for the run and plan kinds: run_new, run_close, run_gate, task_add, map_add, task_solo, plan_workers, ledger_import, ledger_gc

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_handlers_run && cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_run.rs`, `crates/horch-core/tests/fleetd_handlers_run.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

fleetd applies the request kinds `run_new`, `run_close`, `run_gate`, `task_add`, `map_add`, `task_solo`, `plan_workers`, `ledger_import`, `ledger_gc` with the same rows and the same words as the commands of today.

## Requirements

- RR-5 (fleetd applies the change; the command only sends it)
- RR-8 (each kind applies at most 1 time)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A handler is 1 function `fn(&Transaction, &Value) -> Result<Value>` in the map of ticket 12. It writes the rows that the `horch` command writes today, through the same functions of `crates/horch-core/src/taskledger/` and `crates/horch-core/src/fleetd/`, and through `transition()` for each move of a state. It does no effect: an effect is an intended row that the pass carries out after the commit (A24).
- Find the writes of each command: read its CLI code and list each `immediate(` call and each function that it calls. The payload of a kind holds the arguments of the command, not the rows.
- A large input travels as a path with its sha256 (Open question Q15): the handler reads the file with the size limit of the kind and refuses the request when the sha256 differs.
- The result of a handler is a JSON object with the values that the command prints today, so that the command prints the same words.
- Today the client-side check of `HORCH_ROLE` refuses some of these commands to a worker. That check stays as a hint for the user, but fleetd does not trust it: the decision of ticket 09 is the rule.
- The code of today: `crates/horch-core/src/taskledger/runs.rs`, `map.rs`, `tasks.rs`, `import.rs`, and the matching arms of `crates/horch/src/cmd/taskledger.rs`.
- `map_add` (`horch ledger add-map`): the payload has the absolute path of `MAP.md` and the sha256 of the map and of each ticket. The handler reads them with the code of `taskledger::map` and keeps the ticket snapshot and its sha256 on each task as today. `--dry-run` reads only and sends no request (ticket 21).
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in the test file of `Own`, 1 for each kind, through `requests::handle` of ticket 12 with a temporary ledger: each test sends the request and asserts the rows that the command of today writes (copy the assertions of the existing CLI test of that command). Add 1 test for each kind that sends the same `request_id` 2 times and asserts 1 effect. Tests:
   - `run_new_writes_the_run_row_of_today`
   - `run_close_and_run_gate_write_the_rows_of_today`
   - `task_add_queues_1_task_with_its_ticket_snapshot`
   - `map_add_queues_each_row_of_the_map`
   - `map_add_with_a_changed_ticket_sha_is_refused`
   - `task_solo_creates_the_solo_run_when_absent`
   - `plan_workers_writes_the_plan_of_today`
   - `ledger_import_and_gc_write_the_rows_of_today`
   - `each_kind_twice_with_1_request_id_has_1_effect`
2. Write 1 handler for each kind and register it in the map.

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
- Each test passes.
- Each kind of this ticket has a handler, and no handler calls a process, a herdr command or git.
