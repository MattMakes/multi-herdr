# 10: The pass of Phase 3: the worktree facts in part 1, the reconciler in part 2, the worktree_list job each period, and 1 git writer for each repository

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test fleetd_pass --test fleetd_pass_p3 && cargo test -p horch && cargo test -p horch-e2e --test taskledger --test p1_pass && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch/src/cmd/fleetd.rs`, `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch-core/tests/fleetd_pass_p3.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" (the pseudo code of the pass: `scan_worktrees(budget)` and `reconcile_worktrees(tx, trees)`; "A pass has 3 parts"; "Stops come before launches"; "Long work is a job"; "The fact scans of part 1 have a budget also"), "Worktrees: leased, reconciled, never lost" ("The reconciler compares three views": a `worktree_list` job each 60 s), "Features, landing and integration" ("1 git writer for each repository"), RR-10, FM-12.

## Goal

Each pass of fleetd runs the worktree reconciler of ticket 09 with the facts of part 1, queues 1 `worktree_list` job for each repository of the fleet in each period, and starts at most 1 job that changes git state for 1 repository at a time. No git call and no folder walk runs in the pass: a slow `git worktree add` never delays the heartbeat.

## Requirements

- **RR-10**: within the scan budget of each pass, and with a `worktree_list` job each `worktree_list_period_s`.
- **FM-12** (kept): the pass never waits for a worktree job, and its heartbeat stays on time while a `git worktree add` of 60 s runs.
- 1 git writer for each repository (Q12).

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q4, Q9, Q11 and Q12 apply here).
- The pass of Phases 1 and 2: `Pass::run`, `read_facts`, `decide`, `check_invariants`, `intended_effects` and `do_effects` in `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`; the port `Outside` with `worktree_folders(&self, conn)`; `ScanKind::WorktreeFolders` and its cursor in `pass_cursors` (`fleet_runtime/domain/budget.rs`, `adapters/scans.rs`). The pass is built in `crates/horch/src/cmd/fleetd.rs` (the daemon, ticket 23 of fleetd-p1, with the Phase 2 wiring of ticket 14 of fleetd-p2) and in `crates/horch/src/cmd/taskledger_watch.rs` (`horch ledger watch --once`, 1 pass in the foreground, `docs/tickets/fleetd-p1/MAP.md` Q11).
- Part 1: `Outside::worktree_folders` gives the path of each row that is not `removed`, and each folder under `<project>/.worktrees/` of each run that is not `done` or `abandoned` (Q4). The scan of Phase 1 reads each with its cursor and its time budget. The last `passed` `worktree_list` run of each repository is read from `jobs` (no git call).
- Part 2, in `decide`, in this order: the observations and the orphan marks of `reconcile_worktrees` (ticket 09) come with the other observations and before the dispatch and the job queues ("Stops come before launches"); then `check_i6` with the other invariants; then 1 `worktree_list` job for each repository whose period on the fleet clock started and that has no run for this period (input key of ticket 05).
- Part 3: `intended_effects` takes a due job of kind `worktree_add`, `worktree_remove` or `rescue` only when `git_writer_busy(tx, repo)` (ticket 05) is false and no other job of those kinds for that repository is already in the effects of this pass.
- The repositories of the fleet: the `project_root` of each run that is not `done` or `abandoned` (`runs` table).
- Do not run the pass on the repository of this checkout: each test makes its own temporary repository and ledger.

## Steps

1. Write the tests first, in `crates/horch-core/tests/fleetd_pass_p3.rs`. Each fails before the code exists:
   - `slow_worktree_add_job_keeps_the_pass_under_its_time_budget`: a `worktree_add` whose fake git sleeps 60 s; each pass in that time ends within `pass_time_budget_s` and writes its heartbeat.
   - `worktree_scan_continues_at_its_cursor_after_a_restart`: 3 times more folders than the scan budget; a new `Pass` (a restart) starts after the saved cursor, and each folder is seen within 3 passes.
   - `worktree_list_job_is_queued_once_per_period_per_repository`.
   - `only_1_git_writing_job_of_1_repository_starts_in_1_pass`.
   - `pass_adopts_a_hand_made_worktree_within_1_list_period` (RR-10).
   - `pass_marks_the_tree_of_a_dead_owner_orphaned_before_any_dispatch`.
2. Write the code. Keep the tests of `crates/horch-core/tests/fleetd_pass.rs` and of `crates/horch-e2e/tests/p1_pass.rs` (from ticket 28 of fleetd-p1) green with no change.
3. Run the Check. Commit.

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

The Check passes. Each pass reconciles the worktrees from the facts of part 1 with no git call, a `worktree_list` job runs once in each period for each repository, and 1 repository never has 2 git-writing jobs running.
