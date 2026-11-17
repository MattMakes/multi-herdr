# 48: Retire the worker request kinds: spawn, worker_new, worker_pid, worker_reap, orchestrator_claim, orchestrator_stop and task_unblock; keep worker_stop and task_fallback

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch --test p6_retired_worker_commands && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_task.rs`, `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/cmd/taskledger_spawn.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch/tests/p6_retired_worker_commands.rs`

Spec: `docs/specs/fleetd/design.md` (Architecture and roles (Each request kind has a class); REQUIRED item 2).

## Goal

The kind catalog is the table of the design plus the kinds that MAP.md keeps (Q1), and each old command names what you do in its place.

## Requirements

- REQUIRED item 2 (part 2)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Retired here: `spawn` (`horch spawn`, `horch ledger spawn`), `worker_new`, `worker_pid`, `worker_reap`, `orchestrator_claim`, `orchestrator_stop`, `task_unblock`. fleetd dispatches, launches and records each attempt itself; `horch task requeue` replaces `unblock`.
- Kept (answer of the orchestrator to Q1, MAP.md): `worker_stop` (`horch ledger stop` stops 1 worker; the design has no other command for it) and `task_fallback` (`horch ledger fallback` gives a task to another persona; the design has no other command for it), with `ledger_waive`, `ledger_import`, `ledger_gc`, `worktree_scratch` and `worktree_triage`.
- The internal spawns of fleetd (attempts, reviewers of the review scripts, resolvers) do not use the `spawn` request kind. Check it with a test.
- `horch spawn <persona>` exits 1 and prints its line of the table "Old flow and new".
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: 1 test per retired command, `kind_catalog_is_the_table_of_the_design_and_the_kept_kinds`, `ledger_stop_still_stops_1_worker`, `ledger_fallback_still_moves_a_task`, `fleetd_review_spawn_does_not_use_the_spawn_kind`. They fail because the kinds still exist.
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
