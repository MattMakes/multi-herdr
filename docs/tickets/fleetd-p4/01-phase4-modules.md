# 01: The module files of Phase 4: the integration context, the new files of task_execution, worker_profiles and worktree_custody, and their rows in the fitness rules and the architecture doc

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo build -p horch-core && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/mod.rs`, `crates/horch-core/src/fleetd/integration/`, `crates/horch-core/src/fleetd/task_execution/domain/mod.rs`, `crates/horch-core/src/fleetd/task_execution/application/mod.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/mod.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/mod.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/mod.rs`, `crates/horch-core/src/fleetd/worktree_custody/domain/mod.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/mod.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/mod.rs`, `crates/horch-core/src/fleetd/task_execution/domain/task_files.rs`, `crates/horch-core/src/fleetd/task_execution/domain/dispatch.rs`, `crates/horch-core/src/fleetd/task_execution/domain/result.rs`, `crates/horch-core/src/fleetd/task_execution/domain/verification.rs`, `crates/horch-core/src/fleetd/task_execution/domain/structural.rs`, `crates/horch-core/src/fleetd/task_execution/domain/repair.rs`, `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/src/fleetd/task_execution/application/launch.rs`, `crates/horch-core/src/fleetd/task_execution/application/ingest.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/src/fleetd/task_execution/application/review.rs`, `crates/horch-core/src/fleetd/task_execution/application/repair.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/task_files.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/result_reader.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/ingested_store.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/verdict_file.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/stop_gate.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/launch_env.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs`, `crates/horch-core/src/fleetd/worktree_custody/domain/rescue_class.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/rescue_refs.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/gate_tree.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/rescue_refs_git.rs`, `crates/horch-core/tests/arch_scan.rs`, `docs/architecture.md`

Spec: `docs/specs/fleetd/design.md`, "Architecture and roles" (Module owners inside the 1 binary), "Migration plan" item 6; `ddd/08-code/implementation-plan.md` Slice 4 (Files/modules); `ddd/08-code/integration/design.md`.

## Goal

Each file that a Phase 4 ticket owns exists before the tickets of wave 2 start, so that 2 tickets of 1 wave never own 1 file. The new context `integration` exists with `domain/`, `application/` and `adapters/`, and the fitness rules know it. Each new file holds only its module doc comment (1 to 3 lines that say which ticket fills it) and compiles.

## Requirements

- No requirement id becomes green here. This ticket gives the files of the tickets 03 to 47 (MAP.md, Q1).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Follow ticket 01 of `docs/tickets/fleetd-p3/` (the worktree_custody files) and ticket 01 of `docs/tickets/fleetd-p2/`: the same shape, the same doc comment style.
- The new context `crates/horch-core/src/fleetd/integration/` owns the tables `features`, `branches`, `bundles`, `bundle_members`, `gate_runs` and `remote_tips` (design, "Module owners": "Integration owns branches and merges"). Add these tables to the owner list of the foreign-table fitness rule in `crates/horch-core/tests/arch_scan.rs` (Phase 1 Q13 made that rule). `ingested` belongs to task_execution.
- `adapters/executors.rs` of integration is the router of the job kinds `bundle_build`, `fast_gate`, `full_gate`, `fetch`, `push` and `rebase`: 1 `pub fn run(kind, ...) -> Result<Outcome>` that calls `exec_bundle::run`, `exec_gate::run`, `exec_fetch::run`, `exec_push::run` and `exec_rebase::run`. Each of these 5 functions returns today the same "no executor" outcome as `run_job` in `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`. Ticket 03 routes `run_job` to this router; tickets 27, 28, 37, 38 and 41 fill 1 file each.
- Add 1 row for the context `integration` in `docs/architecture.md`, in the table that `crates/horch-core/tests/architecture_doc.rs` reads.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Add the fitness-rule entries first, and see `cargo test -p horch-core --test arch_scan` fail for the missing module.
2. Make each file of `Own`, with its `pub mod` line in the parent `mod.rs`. A file has its doc comment and nothing else, except `adapters/executors.rs` and the 5 `exec_*.rs` stubs of integration.
3. Run the Check. Commit the exact files.

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

Self-hosting: this phase changes `horch spawn`, the claim and the finish steps. The build fleet itself uses the installed `horch` for these. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).

## Done when

The Check passes. Each file of `Own` exists and is in the module tree. `arch_scan` knows the context `integration` and its tables.
