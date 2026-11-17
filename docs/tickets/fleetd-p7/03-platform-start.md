# 03: The platform start function: 1 type that starts each process that is not a fleet process, and always removes each forbidden name

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-platform --locked && cargo test -p horch-core --test arch_scan -- arc_88 && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-platform/src/`, `crates/horch-platform/tests/start.rs`, `crates/horch-core/src/harness/launch.rs`

Spec: `docs/specs/fleetd/design.md` (SR-7; "Worker profiles and adapters" (1 launcher for each process); Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode").

## Goal

`crates/horch-platform/src/start.rs` gives the only way, outside the launcher, to start a process. A process that it starts never has `ANTHROPIC_API_KEY` or another forbidden name.

## Requirements

- SR-7 (part): the one door that the rule of ticket 31 allows next to the launcher (MAP.md Q1).

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The rule of the move (answer of the orchestrator, MAP.md Q1): each direct process start of production code goes through the launcher (`crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`) or through the platform start function of ticket 03 (`crates/horch-platform/src/start.rs`). Test binaries and code under `#[cfg(test)]` are not production code. A process that fleetd, the wrapper, a job, docs sync or the conformance suite starts uses the launcher. Each other start (an operator command, the installer) uses the platform start function. Both remove each forbidden name.
- Today the forbidden list is `FORBIDDEN_ENV` in `crates/horch-core/src/harness/launch.rs:271`, and `scrub_child_env` in `crates/horch-core/src/runtime/process.rs:142` removes it. The test `arc_88_children_are_scrubbed_in_one_place` (`crates/horch-core/tests/arch_scan.rs:712`) keeps the removal in `runtime/` and `harness/launch.rs`.
- Choice (MAP.md Q1): the list moves to `horch_platform::start::FORBIDDEN_ENV`. `harness/launch.rs` re-exports it, so that each caller of today compiles. Update `arc_88` so that the removal is in `horch-platform/src/start.rs` and the launcher only.
- The type: `start::Command`, a wrapper of `std::process::Command` with the builder methods that callers use (`arg`, `args`, `env`, `envs`, `env_remove`, `current_dir`, `stdin`, `stdout`, `stderr`), and `spawn`, `output` and `status`. Each of the 3 removes each forbidden name just before the start, so an `env` call after a removal cannot put it back. `std::process::Command::new` is written only in this file in the platform crate. The other platform concerns (`group.rs`, `sandbox.rs`, `service.rs`, `windows/path.rs`) take or give this type; an access to the inner `std` command is `pub(crate)`.
- Windows code cannot be compiled on this Mac. Keep it small, read it 2 times, and name each Windows risk in your done summary. `cargo check --target x86_64-pc-windows-msvc` only if the target is installed.

## Steps

1. Write the tests first in `crates/horch-platform/tests/start.rs`: `forbidden_name_never_reaches_the_child_of_spawn`, `of_output`, `of_status` (each sets the name in the parent and with `env` on the command, then reads the environment of the child), `env_after_env_remove_cannot_add_a_forbidden_name`. They fail before the type exists.
2. Write `start.rs`. Move the 2 direct starts of the platform crate (`service.rs:181`, `windows/path.rs:90`) onto it. Update `arc_88`.
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
- `rg -n "process::Command::new|Command::new" crates/horch-platform/src` finds only `start.rs` and test code.
