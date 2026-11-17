# 42: A rebase that conflicts opens 1 resolver task in the run, with the conflict in zdiff3 form

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test rebase_resolver_task && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/application/branches.rs`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/tests/rebase_resolver_task.rs`

Spec: `docs/specs/fleetd/design.md`, "Between features" (Continuous rebase: a conflict becomes a resolver task; Starting values: a resolver task covers at most 100 changed lines or 2 files, else fleetd splits it by file).

## Goal

When the rebase job of a feature ends with a conflict, fleetd adds 1 resolver task to the run (a code kind, with a ticket text that fleetd writes: the conflict files, the 2 tips, and the tests of both sides), split by file over `resolver_max_lines` or `resolver_max_files`. The task lands on the feature branch as any task.

## Requirements

- 1 conflict gives 1 resolver task (or 1 for each file group), never 2 for the same pair of tips.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The task is added with today's `tasks::add` path (`crates/horch-core/src/taskledger/tasks.rs`). The keys `resolver_max_lines` (100) and `resolver_max_files` (2) come from `fleet.toml` (ticket 03).
- This file `application/branches.rs` is shared with ticket 44: ticket 44 runs after this one.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `rebase_conflict_adds_1_resolver_task`, `same_conflict_twice_adds_no_second_task`, `conflict_over_2_files_is_split_by_file`.
2. Write the resolver task.
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

The Check passes. A rebase conflict always becomes work in the run, never a silent wait.
