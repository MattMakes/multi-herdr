# 31: The replan of a failed task: the sub-orchestrator of the feature replaces the task or sends it back 1 time, else it is escalated

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test replan && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/replan.rs`, `crates/horch-core/src/fleetd/task_execution/application/planned_tasks.rs`, `crates/horch-core/tests/replan.rs`

Spec: `docs/specs/fleetd/design.md` (Supervision (The task state machine, `failed`); Requirements RR-6; Failure mode FM-21).

## Goal

Use case `ReplanTask`: a `failed` task gets 1 replan by the sub-orchestrator of its feature within `replan_limit_s`, then it is `escalated`.

## Requirements

- FM-21 (bounded attempts, 1 replan, then escalation)
- The owner of a failed task is the sub-orchestrator (replaces Q12 of fleetd-p2)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- The sub-orchestrator writes `ideas/I-NNNN/replan-<task key>.yaml`: either `replace:` with new task entries (validated by the same `PlanValidator`, ticket 12 refuses a replan that leaves a judgment task without judge), or `retry: <reason>`.
- `replace`: in 1 transaction, the old task follows A cancel and the new tasks are added through `AddPlannedTasks` (`planned_tasks.rs`). `retry`: the task goes to `ready` with its attempt count at 0. Each adds 1 to `replans`.
- A task that reaches `failed` with `replans` = 1, or a `failed` task past `replan_limit_s` on the fleet clock, goes to `escalated` with 1 needs-you item.
- Today (Q12 of fleetd-p2) the orchestrator gets 1 wake, and a requeue of a `failed` task is its 1 replan. Phase 6 replaces that stand-in.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `failed_task_is_replaced_in_1_transaction`, `failed_task_retry_resets_attempts`, `second_failure_after_a_replan_escalates`, `failed_task_past_replan_limit_escalates`, `replan_that_leaves_a_judgment_task_without_judge_is_refused_here_too`. They fail because the use case is empty.
2. Write `ReplanTask` and the replace path of `AddPlannedTasks`.
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
