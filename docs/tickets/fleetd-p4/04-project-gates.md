# 04: The project declares its gates: [gates] with fast and full in the project fleet.toml, the pr and remote settings, the hold "missing fast gate", and the gates of this repository

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::integration::domain::project && cargo test -p horch-core --test integration_project && cargo test -p horch-core --lib fleetd::fleet_runtime::application::holds && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/domain/project.rs`, `crates/horch-core/src/fleetd/integration/adapters/project_file.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs`, `crates/horch-core/tests/integration_project.rs`, `fleet.toml`

Spec: `docs/specs/fleetd/design.md`, "Between features" (Two declared gates; The pull request; `remote = none`), "Holds on dispatch are visible", OR-1.

## Goal

A project declares its fast gate and its full gate in 1 format. fleetd reads it. A project with no fast gate holds new dispatch with a cause that you can read, and 1 needs-you item. This repository declares its gates.

## Requirements

- Required item 2 of the phase: the format is the file `fleet.toml` at the root of the project, with the table `[gates]` and the 2 string keys `fast` and `full` (the format of `declare_gates` in `crates/horch-e2e/tests/p0_landing.rs`). Optional keys at the top level: `pr` = `gh` or `manual` (default `gh`), and `remote` = the remote name or `none` (default `origin`).
- Answer of the orchestrator (Q-C): a project with no `[gates].fast` gets the dispatch hold `missing fast gate` and 1 needs-you item for the project. No task is lost or cancelled: a task in flight goes on, and only new dispatch waits.
- The gates of this repository (Q-C, a choice that the operator can change): `fast = "cargo test --workspace --locked --lib --bins"`, `full = "cargo test --workspace --locked"`.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The project `fleet.toml` is not the global `fleet.toml` of the fleet root (Phase 1 Q4). The project file is committed in the repository, and fleetd reads it at the tip of the feature branch (`git show <tip>:fleet.toml`, through the git gateway of ticket 23; this ticket gives the parser and a reader port, and a test adapter that reads a folder).
- The hold: `holds.rs` (Phase 1 ticket 19, Phase 3 ticket 16) has the global holds. Add the hold of a project with its cause text `missing fast gate: add [gates] fast to fleet.toml in <project>`. It holds only the dispatch of tasks of that project. The needs-you key is `missing_fast_gate:<project id>`, resolved when the gate is declared.
- Unknown keys in `[gates]` are an error that names the key. An empty command is an error.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `gates_parse_fast_and_full`, `missing_fast_gate_is_none`, `unknown_gate_key_is_refused`, `pr_and_remote_defaults`, `remote_none_is_read`; in `holds.rs`, `project_with_no_fast_gate_holds_its_dispatch_with_its_cause` and `the_hold_ends_when_the_gate_is_declared`.
2. Write the parser, the reader port and the hold. Add `fleet.toml` at the repository root with the 2 gates of this repository.
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

The Check passes. A project with no fast gate shows the hold `missing fast gate` and 1 needs-you item, and its tasks in flight are not changed.
