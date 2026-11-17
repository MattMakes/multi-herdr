# 37: Stacks: a trial merge that conflicts puts the younger feature on the older one, 1 time, with 1 resolver task that lands on the younger feature

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test stacks && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/domain/stack.rs`, `crates/horch-core/src/fleetd/integration/application/stacks.rs`, `crates/horch-core/tests/stacks.rs`

Spec: `docs/specs/fleetd/design.md` (Between features (Stack on evidence; The pull request); `ddd/08-code/integration/design.md` (`StackPlanner`, `StackFeature`)).

## Goal

`StackPlanner` turns a conflict fact into 1 stack edge and 1 resolver task, and never 2 for the same pair.

## Requirements

- Stacks (new work of Phase 6)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- When the trial merge of 2 in-flight features conflicts, fleetd sets `stacked_on` of the younger (later `created_at`) to the older, 1 time.
- The resolver task: a fresh agent, the conflict in `zdiff3` form, the requirements of both features for those files, and the unit tests of both. It makes 1 commit for each requirement and lands on the younger feature's branch. It is a task of a code kind, so it follows the code path.
- FR-12: the resolver task belongs to an idea: the youngest idea in flight of the younger feature (MAP.md, Q17). It is added through `AddPlannedTasks` (ticket 14) with a plan entry that fleetd writes.
- A resolver task covers at most `resolver_max_lines` and `resolver_max_files` (Phase 4); else fleetd splits it by file.
- The pull request of a stacked feature has the branch of the lower feature as its base (ticket 38 moves the base).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `conflicting_features_stack_younger_on_older_once`, `given_2_features_touching_1_file_when_trial_merge_then_younger_stacked_and_1_resolver_task`, `resolver_task_belongs_to_an_idea_of_the_younger_feature`, `large_conflict_is_split_by_file`. They fail because the files are empty.
2. Write `StackPlanner` and the use case.
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
