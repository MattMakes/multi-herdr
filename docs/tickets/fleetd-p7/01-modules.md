# 01: The module files of Phase 7 in worker_profiles and task_execution, and their rows in the fitness rules and the architecture doc

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** light
**Check:** `cargo build -p horch-core --locked && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/domain/mod.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/mod.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/mod.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/conformance.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/health.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/docs_jobs.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/conformance.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/health.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/docs_jobs.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/docs_sync.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/drift_idea.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/docs_dir.rs`, `crates/horch-core/src/fleetd/task_execution/domain/mod.rs`, `crates/horch-core/src/fleetd/task_execution/domain/context_limit.rs`, `docs/architecture.md`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode"; "Docs sync and adapter conformance"; "Worker profiles and adapters").

## Goal

Each new module of Phase 7 exists as a file with a module doc, so that the tickets of 1 wave own different files.

## Requirements

- No requirement id. This ticket prepares the files for FR-8, FR-9, FM-19 and FM-20.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- `ddd/08-code/implementation-plan.md`, Slice 7, names `fleetd/worker_profiles/application/commands.rs` (`RunConformanceTests`, `MarkProfileHealthy`) and `adapters/` (`CliConformanceRunner`). The design wins where they do not agree. Choice (MAP.md Q17): 1 file for each concern, as in Phases 2 to 6.
- The files and their concern: `domain/conformance.rs` (the 10 checks, the verdict of 1 check, the form of `conformance.json`); `domain/health.rs` (the health of a profile from its events); `domain/docs_jobs.rs` (when fleetd queues a `docs_sync` or a `conformance` run); `application/conformance.rs` (run the suite through a port); `application/health.rs` (mark a profile unhealthy or healthy); `application/docs_jobs.rs` (queue the 2 kinds through `jobs::queue`); `application/docs_sync.rs` (the port of the docs sync executor, because horch-core has no HTTP crate, NFR-06); `application/drift_idea.rs` (a drift becomes an idea); `adapters/conformance_runner.rs` (the real launch of each check through the launcher); `adapters/docs_dir.rs` (the files under `<fleet root>/docs/<cli>/<version>/`); `task_execution/domain/context_limit.rs` (the outcome `context_limit` and its loop guard).
- The fitness rules in `crates/horch-core/tests/arch_scan.rs` (`fitness_domain_imports_nothing_outward`, `fitness_no_context_touches_foreign_tables`) and the rows of `docs/architecture.md` (`crates/horch-core/tests/architecture_doc.rs`) must know each new file.

## Steps

1. Make each file of `Own` with a module doc of 2 to 5 lines that names its concern and the ticket of this map that fills it. Add the `mod` lines.
2. Add the rows that the guard tests need. Run the Check. Commit the exact files.

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
- Each file of `Own` exists and names its concern.
