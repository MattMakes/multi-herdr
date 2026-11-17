# 16: The worktree disk budget: over it, fleetd removes landed and rescued trees of finished runs, oldest first, and holds new dispatch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test worktree_disk --test fleetd_pass --test fleetd_pass_p3 && cargo test -p horch && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/domain/disk.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/disk.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/health.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/tests/worktree_disk.rs`

Spec: `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never lost" ("Disk"), FM-28, OR-1, "The numbers of the design" (row "Worktree disk budget"), "Supervision, liveness and reconciliation" ("holds on dispatch").

## Goal

The fleet never fills the disk with worktrees. The size of each tree comes from the `worktree_list` job. When the total is above `worktree_disk_budget_gb`, fleetd removes `landed` and `rescued` trees of runs that are `done` or `abandoned`, oldest first, until the total is under the budget. It never removes an `active` tree for space. If that is not enough, it holds new dispatch and opens 1 needs-you item.

## Requirements

- **FM-28**: size is tracked on each git scan; over the budget, fleetd removes landed and rescued trees of finished work and pauses dispatch before it would touch an active one.
- **OR-1** (the worktree part): new dispatch stops before the disk is full.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q8, Q9 and Q20 apply here).
- The facts: `bytes_on_disk` of each row (refreshed by the `worktree_list` result, tickets 05 and 09), `worktree_disk_budget_gb` (ticket 04), the state of each run (`runs.status`: `active`, `done`, `abandoned`).
- The decision, in `domain/disk.rs` (pure): from the rows and the budget, the list of trees to remove, oldest `created_at` first: only `landed` and `rescued` trees whose run is `done` or `abandoned` (Q8: the stand-in for an idea `merged` or `cancelled` until Phase 6), and only a `rescued` tree with a task. Never `active`, `landing`, `created`, `reserved` or `orphaned`. If the total stays above the budget after each such tree, the decision also says "hold".
- `application/disk.rs`: `disk_budget(tx, clock) -> Report`, called in part 2 of the pass after `reconcile_worktrees`. It calls `queue_remove` (ticket 07) for each tree of the decision that has no active job. The guard of ticket 07 still checks each tree at its removal. In report-only mode (until ticket 18) it reports the trees and queues nothing, as ticket 09 does.
- The hold: add a variant of `Hold` in `crates/horch-core/src/fleetd/fleet_runtime/domain/health.rs`, for example `Hold::WorktreeDisk { cause }`, a global hold. `holds()` and `record_hold_changes` in `fleet_runtime/application/holds.rs` give and record it, and 1 needs-you item opens with the total and the budget (`worktree_disk:<repo>`); it is resolved when the total is under the budget. The dispatch points of Phase 1 read the holds (`docs/tickets/fleetd-p1/MAP.md` Q16: `horch spawn` and `horch ledger spawn`); check that they refuse a spawn with the new hold. If the file that reads the holds is not in `Own`, send a QUESTION.
- `pass.rs` (ticket 10 wired the reconciler): add the call of `disk_budget` in part 2 and the hold in part 1.

## Steps

1. Write the tests first, in `crates/horch-core/tests/worktree_disk.rs` and the module tests of `domain/disk.rs`. Each fails before the code exists:
   - `over_budget_removes_landed_and_rescued_trees_of_finished_runs_oldest_first`.
   - `over_budget_never_removes_an_active_tree_and_holds_dispatch`: an active tree with dirty and untracked files and a commit stays with each one.
   - `over_budget_never_removes_a_tree_of_an_active_run`.
   - `hold_opens_1_needs_you_item_and_resolves_under_the_budget`.
   - `report_only_mode_queues_no_removal_for_space`.
2. Write the code. Run the Check. Commit.

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

Self-hosting: each test makes its own temporary git repository and temporary folders (build rule 1). No test and no worker runs the reconciler, a rescue, a cleanup or a `git worktree remove` on the repository `/Users/MASCOTT/projects/mh/multi-herdr` or on its `.worktrees/` folder.

## Done when

The Check passes. Over the budget, fleetd queues the removal of only `landed` and `rescued` trees of finished runs, oldest first, never an active tree, and holds dispatch with 1 needs-you item when that is not enough.
