# 08: Push dispatch: 1 function binds a ready task to a new attempt before its pane exists, and fleetd decides which ready tasks to dispatch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --test push_dispatch && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/dispatch.rs`, `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/tests/push_dispatch.rs`

Spec: `docs/specs/fleetd/design.md`, "The task state machine" (`ready`, `leased`), FR-5, FM-2, FM-14, "Holds on dispatch are visible", I4; `docs/specs/fleetd/preservation-04-behavior.md` rows L1, L2, L3, L6.

## Goal

A task is bound to an attempt in 1 transaction before any pane or process exists: the task goes `ready -> leased`, the attempt row is `launching` with `via = push`, its worktree is the tree of the task (reused) or a new `reserved` row, and `task.yaml` and `START.md` are written after the commit. fleetd decides which ready tasks to dispatch in each pass, and `horch spawn` calls the same bind.

## Requirements

- FR-5: the supervisor assigns tasks to workers; a worker never searches for work. The bind is the only code that chooses a task.
- FM-2: the task is bound and `task.yaml` exists before the launch.
- FM-14: the bind is a compare-and-set on `ready`; 2 binds of 1 task give 1 attempt.
- L2 order (priority, then seq; a target task first; a replacement takes its predecessor's task first), L3 (a retry goes first), L6 (no 2 running tasks of 1 run own overlapping paths; an exclusive task runs alone; review tasks never wait), the holds of Phases 1 to 4 (health, pacing, `not_before`, `missing fast gate` of ticket 04) and the check sandbox rule: a persona whose check sandbox is NotSupported is not dispatched (ticket 18 gives the port `CheckSandboxes::supported(persona)`; this ticket calls it).
- The lease and the start timeout start at the launch, not at the bind (task state `leased`: "While its worktree is `reserved`, the attempt waits for the `worktree_add` job").

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Today's choice of a task is `pick_task` and `take_task` in `crates/horch-core/src/taskledger/tasks.rs` (claim, L1, L2, L6). Move the rules, not the code path: `domain/dispatch.rs` holds the pure choice (`fn choose(ready: &[ReadyTask], running: &[RunningTask], holds: &Holds) -> Vec<Choice>`), and `application/dispatch.rs` holds `bind(tx, task, persona, worker) -> Bound` and `dispatch_due(tx, budget) -> Vec<Bound>`. Ticket 09 makes `horch spawn` call `bind`; ticket 10 launches what `dispatch_due` bound; the claim stays for old attempts only.
- The worktree: when the task has a tree (Phase 3 ticket 11, the next attempt reuses it), the bind names it and moves its owner to the new attempt; else it writes the `reserved` row and queues the `worktree_add` job (Phase 3 ticket 08, `worktree_custody/application/create.rs`). The path and the branch are known at the bind, so `task.yaml` can name them.
- The persona: the task's `claimable_by` (Phase 2 fallback) else its primary persona. The worker row: a new `workers` row of that persona, written in the same transaction, as `horch spawn` writes it today (`taskledger_spawn::prepare`).
- After the commit: write `task.yaml` and `START.md` (ticket 07). A crash between the commit and the write: the next pass writes the 2 files of each `launching` attempt that has none (safe to repeat, the writer never rewrites).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/push_dispatch.rs`: `bind_moves_ready_to_leased_with_1_launching_attempt`, `fm14_2_binds_of_1_task_give_1_attempt`, `pres_04_l2_target_and_retry_go_first`, `pres_04_l6_overlapping_paths_are_not_dispatched_together`, `held_task_is_not_bound_and_shows_its_hold`, `not_supported_check_sandbox_persona_is_not_bound`, `next_attempt_is_bound_to_the_same_worktree`, `task_files_of_a_launching_attempt_are_written_after_a_crash`, `lease_starts_at_the_launch_not_at_the_bind`.
2. Write `domain/dispatch.rs` and `application/dispatch.rs`.
3. This ticket moves a ref, lands, pushes, deletes a branch or a ref, or changes who runs the finish steps. It has a crash test: a fault point before the commit of the intended row, and after the commit and before the effect. In each case the next pass completes or fails the effect 1 time, and no tip and no work is lost.
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

Self-hosting: this phase changes `horch spawn`, the claim and the finish steps. The build fleet itself uses the installed `horch` for these. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).

## Done when

The Check passes. A task is bound by 1 function, before its pane exists, and `task.yaml` exists before the launch.
