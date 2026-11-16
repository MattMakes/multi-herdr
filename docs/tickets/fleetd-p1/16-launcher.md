# 16: The 1 launcher for each process that fleetd starts

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::worker_profiles && cargo test -p horch-core --test arch_scan`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/application/ports.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`

Spec: `docs/specs/fleetd/design.md`, SR-7, "Worker profiles and adapters" (1 launcher for each process), "Architecture and roles" (row Platform layer: "It owns the process start. Only the launcher calls it"), "Preservation analysis" row 05 E-10, 06 S1, FM-13.

## Goal

1 launcher function starts each process of fleetd: an attempt, a job, the restart of fleetd. It applies the environment rule, always removes `ANTHROPIC_API_KEY` (also for a process with no profile), starts the process in its own process group or job object, and refuses to start inside a ledger transaction.

## Requirements

- **SR-7** (part): Phase 1 gives the launcher, and each process of fleetd starts through it. SR-7 becomes green in Phase 7, with the `arch_scan` rule `fitness_no_process_start_outside_the_launcher` (not in this ticket).
- **FM-13** (part): each new attempt starts through the launcher, which removes `ANTHROPIC_API_KEY`.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Today: `scrub_child_env` in `crates/horch-core/src/runtime/process.rs:190` cleans a child environment, and `arc_88_children_are_scrubbed_in_one_place` keeps the removal of the key in `runtime/` and `harness/launch.rs`. The launcher calls `scrub_child_env`. Do not write a removal of the key by hand.
- The process group: the platform group concern of Phase P (`group::new_group(&mut Command)`, `group::detach`; read the module doc of `crates/horch-platform/src/group.rs`).
- Proposed API, in `application/ports.rs` the port `ProcessHost` and the types `LaunchSpec { program, args, cwd, env_rule, detach: bool }` and `Launched { pid, pid_start }`; in `adapters/launcher.rs` the implementation `Launcher` that uses the platform.
- The guard "no process start inside a transaction": the ledger store of ticket 11 opens transactions. Ticket 11 gives `in_door_transaction()` in `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`. Make `launch` return an error (and `debug_assert!`) when it is true. The test name of the design is `start_agent_process_inside_transaction_panics_in_debug` (Slice 4); in Phase 1 the test is `launch_inside_a_transaction_is_refused`.
- The environment rule of a profile comes with the profile adapters of Phase 4. In Phase 1, the env rule is the rule of today (`scrub_child_env`) plus the variables that the caller gives.

## Steps

1. Write the tests first, in `adapters/launcher.rs`. Each fails before the code exists:
   - `launcher_removes_the_forbidden_key_for_a_process_with_no_profile`: start `/usr/bin/env` (`#[cfg(unix)]`) with the key in the parent `Command`, and read its output.
   - `launcher_starts_each_process_in_its_own_group` (`#[cfg(unix)]`).
   - `launch_inside_a_transaction_is_refused`.
   - `launched_gives_the_pid_and_its_start_time`.
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

The Check passes. No process of the launcher has `ANTHROPIC_API_KEY`, and each is in its own group.
