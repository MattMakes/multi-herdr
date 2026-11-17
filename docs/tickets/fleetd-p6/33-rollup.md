# 33: The roll-up of an idea and ideas finish clean: in_progress to integrating to merged, and fleetd removes the rescued trees of a merged or cancelled idea

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test idea_rollup && cargo test -p horch-core --test worktree_disk && cargo test -p horch-core --test worktree_reconcile && cargo test -p horch-core --test worktree_reconcile_acting && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/rollup.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/disk.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`, `crates/horch-core/tests/idea_rollup.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (The idea state machine, `in_progress`, `integrating`, `merged`); No work is lost; Failure modes FM-28, FM-39).

## Goal

Use cases `RollUpIdea` and `CloseIdea`. An idea reaches `merged` only when its feature is merged and each of its trees is `removed` or `rescued`, and then fleetd removes its rescued trees.

## Requirements

- FM-39
- "Ideas finish clean" (REQUIRED item 4; Q8 of fleetd-p3)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- `in_progress` -> `integrating`: each task of the idea that is not `cancelled` is `done`, and each task branch that is not `abandoned` is `landed`.
- `integrating` -> `merged`: the feature is `merged` (Phase 4) and each worktree of the idea is `removed` or `rescued`. Then fleetd queues the `worktree_remove` job of each `rescued` tree of the idea; the rescue refs stay.
- Phase 3 used "the run is `done` or `abandoned`" as the stand-in for an idea `merged` or `cancelled` (Q8 of `docs/tickets/fleetd-p3/MAP.md`), in the disk budget (`worktree_custody/application/disk.rs`) and in the reconciler (`reconcile.rs`). Replace each use of the stand-in with the idea state. A rescued tree with no task is still never removed by fleetd.
- A worktree row has no `idea_id` (Q2 of fleetd-p3): find the idea through the task of the row.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/idea_rollup.rs`: `cancelled_task_does_not_block_its_idea`, `idea_is_integrating_when_each_branch_is_landed_or_abandoned`, `idea_is_merged_only_when_each_tree_is_removed_or_rescued`, `rescued_trees_of_a_merged_idea_are_removed_and_refs_stay`, `disk_budget_uses_the_idea_state_not_the_run`. They fail because the use cases are empty and the stand-in is still in place.
2. Write the use cases. Replace the stand-in in `disk.rs` and `reconcile.rs`. Make the tests of Phase 3 in the Check follow it.
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
