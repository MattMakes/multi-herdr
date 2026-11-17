# 07: Move the process starts of the task ledger and vcs onto the launcher or the platform start function

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** light
**Check:** `cargo test -p horch-core --lib taskledger && cargo test -p horch-core --lib vcs && cargo test -p horch-core --test w3_finish_steps && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch-core/src/taskledger/finish/complexity.rs`, `crates/horch-core/src/taskledger/finish/pr.rs`, `crates/horch-core/src/taskledger/triage/apply.rs`, `crates/horch-core/src/taskledger/runs.rs`, `crates/horch-core/src/taskledger/watchdog.rs`, `crates/horch-core/src/vcs/landing.rs`, `crates/horch-core/src/vcs/network.rs`, `crates/horch-core/tests/p7_taskledger_starts.rs`

Spec: `docs/specs/fleetd/design.md` (SR-7; Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode").

## Goal

Each git call and helper start of the task ledger and of `vcs/` goes through the launcher or the platform start function.

## Requirements

- SR-7 (part): the git calls of the jobs and of the old finish steps.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The rule of the move (answer of the orchestrator, MAP.md Q1): each direct process start of production code goes through the launcher (`crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`) or through the platform start function of ticket 03 (`crates/horch-platform/src/start.rs`). Test binaries and code under `#[cfg(test)]` are not production code. A process that fleetd, the wrapper, a job, docs sync or the conformance suite starts uses the launcher. Each other start (an operator command, the installer) uses the platform start function. Both remove each forbidden name.
- Today: `finish/land.rs` 4 sites, `finish/complexity.rs` 3, `finish/pr.rs` 2, `triage/apply.rs` 2, `runs.rs` 2, `vcs/landing.rs` 2, and 1 each in `watchdog.rs` and `vcs/network.rs`.
- A git call of a job runs inside the job process, which fleetd started through the launcher. Choice (MAP.md Q1): the git call uses the launcher too, so that 1 rule covers it.
- `horch_marketplace::git::scrub_repo_env` removes the git repository variables today. Keep that removal for each moved git call.

## Steps

1. List each direct start in the files of `Own` with `rg -n 'Command::new|process::Command' <file>`. Skip code under `#[cfg(test)]`. Write the list in the commit message.
2. Write the tests first: for each folder of `Own`, 1 test that runs 1 moved start with `ANTHROPIC_API_KEY` set in the parent environment, and asserts that the child does not have it. Use a small child that prints its environment (for example `/usr/bin/env` on Unix through the start function). It fails before the move only where the old start did not remove the name; where it passes before the move, it is a regression test, and you say so in the commit message.
3. Move each start behind the launcher or the platform start function. Keep the arguments, the working folder, the environment rule, stdin, stdout, stderr and the exit handling the same. A change of behavior is a fault.
4. Run the Check. Commit the exact files.

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
- No direct start is left in the production code of the files of `Own`.
