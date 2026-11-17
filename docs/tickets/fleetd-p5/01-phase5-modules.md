# 01: The module files of Phase 5, their rows in the fitness rules and in the architecture doc

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** light
**Check:** `cargo build -p horch-core && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/mod.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/mod.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/mod.rs`, `crates/horch-core/src/fleetd/task_execution/domain/mod.rs`, `crates/horch-core/src/fleetd/task_execution/application/mod.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/mod.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/mod.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/mod.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/operator.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/requests.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_run.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_task.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/requests_store.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/token_file.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/job_result_file.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/polite_stops_store.rs`, `crates/horch-core/src/fleetd/task_execution/domain/outbox.rs`, `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/outbox_reader.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/outbox_writer.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/attempt_files.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/operator_boundary.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/sandbox_rules.rs`, `crates/horch-core/tests/arch_scan.rs`, `docs/architecture.md`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

Each new file of Phase 5 exists with its module doc and no code, so that the tickets of 1 wave own different files.

## Requirements

- None directly. This ticket makes the files that the other tickets of Phase 5 own (Open question Q16).

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The 3 contexts of Phase 5 are fleet runtime (the channel, the requests, the token, the request files, the job result file and the polite stops), task execution (the outbox files and the wrapper files) and worker profiles (the operator boundary of a sandbox).
- The pattern is ticket 01 of fleetd-p4 (`docs/tickets/fleetd-p4/01-phase4-modules.md`): 1 file for each concern under `domain/`, `application/` and `adapters/`.
- The fitness rules of the module folders are in `crates/horch-core/tests/arch_scan.rs` and in the `fitness_*` tests of `crates/horch-core/src/fleetd/`. The table owners of Phase 1 Q13 (`docs/tickets/fleetd-p1/MAP.md`) hold: fleet runtime owns `requests` and `polite_stops` from this phase.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the test first: extend the fitness test of the module folders so that it lists each new file below and fails while a file is missing.
2. Make each file of `Own` that does not exist. Each new file has a module doc of 3 to 6 lines that names its concern, the requirement ids, and the ticket of this map that fills it. No code apart from the doc.
   - `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/domain/operator.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/application/requests.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_run.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_task.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/adapters/requests_store.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/adapters/token_file.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/adapters/job_result_file.rs`
   - `crates/horch-core/src/fleetd/fleet_runtime/adapters/polite_stops_store.rs`
   - `crates/horch-core/src/fleetd/task_execution/domain/outbox.rs`
   - `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs`
   - `crates/horch-core/src/fleetd/task_execution/adapters/outbox_reader.rs`
   - `crates/horch-core/src/fleetd/task_execution/adapters/outbox_writer.rs`
   - `crates/horch-core/src/fleetd/task_execution/adapters/attempt_files.rs`
   - `crates/horch-core/src/fleetd/worker_profiles/domain/operator_boundary.rs`
   - `crates/horch-core/src/fleetd/worker_profiles/adapters/sandbox_rules.rs`
3. Add each new file to the `mod` list of its folder.
4. Add the table owners `requests` and `polite_stops` (fleet runtime) to the foreign table rule.
5. Add 1 row for each new file to `docs/architecture.md`, in the table of the fleetd modules.

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
- Each file of the list exists with its module doc, and the fitness test lists it.
- `cargo build -p horch-core` has no new warning.
