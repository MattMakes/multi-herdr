# 47: Retire the run and plan request kinds: horch ledger run new, run close, run gate, add, add-map, task and plan-workers print the new command, and no code path makes a task without an idea

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch --test p6_retired_run_commands && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch-core --lib taskledger::runs`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_run.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch-core/src/taskledger/runs.rs`, `crates/horch/tests/p6_retired_run_commands.rs`

Spec: `docs/specs/fleetd/design.md` (Architecture and roles (Each request kind has a class); Requirements FR-12; REQUIRED item 2).

## Goal

The kinds `run_new`, `run_close`, `run_gate`, `task_add`, `map_add`, `task_solo` and `plan_workers` are retired, and each old command names what you do in its place.

## Requirements

- REQUIRED item 2 (part 1)
- FR-12 (no code path creates a task without an idea)
- Several features in place of 1 non-solo run for each workspace

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- The table "Old flow and new" of `MAP.md` gives the line of each command. Each retired command exits 1 and prints its line, for example `horch ledger run new is retired. Add an idea: horch idea add "<idea>"`.
- `horch ledger add-map --dry-run` stays (ticket 13). `add-map` without `--dry-run` is retired.
- fleetd refuses a request of a retired kind with the reason `retired kind: <new command>` (a request file of an old client, written before the update).
- The rule R1 (`refuse_a_2nd_workspace_run` in `crates/horch-core/src/taskledger/runs.rs`, 1 active non-solo run for each workspace) goes: fleetd makes 1 internal run for each feature (MAP.md, Q5).
- FR-12: each insert into `tasks` goes through `AddPlannedTasks` (ticket 14) or the resolver path of ticket 37; the trigger of ticket 02 refuses a task with no idea. Add `no_code_path_creates_a_task_without_an_idea`: it greps the production code for `INSERT INTO tasks` and finds it only in the allowed files.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch/tests/p6_retired_run_commands.rs`: 1 test per retired command that checks the exit code and the line, `retired_kind_request_file_is_refused_with_the_new_command`, `two_features_in_1_workspace_are_allowed`, `no_code_path_creates_a_task_without_an_idea`. They fail because the commands still work.
2. Retire the kinds, the handlers and the commands.
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
