# 14: Plan commit: fleetd validates plan.yaml, writes plan.verdict.json, and commits each task of a valid plan in 1 transaction

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test plan_commit && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/plan_commit.rs`, `crates/horch-core/src/fleetd/task_execution/application/planned_tasks.rs`, `crates/horch-core/tests/plan_commit.rs`

Spec: `docs/specs/fleetd/design.md` (Requirements FR-3; Ideas (The idea state machine, `ticketing`); Failure modes FM-1, FM-10; Ledger (`tasks`, `task_deps`, `task_covers`)).

## Goal

A plan reaches the ledger all-or-nothing, and only after it passes the validator. No AI creates a task.

## Requirements

- FR-3
- FM-1 (no AI creates tasks)
- FM-10 (the sub-orchestrator reruns ticketing with the errors)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Use case `ValidatePlan` (`ddd/08-code/ideas/design.md`, section 3). It reads `ideas/I-NNNN/plan.yaml` and the tickets that it names with the SR-6 reader, builds `PlanFacts`, and runs `PlanValidator`.
- A refused plan: fleetd writes `plan.verdict.json` with each problem, adds 1 to `phase_attempts`, and the idea stays in `ticketing`.
- A valid plan: 1 transaction writes `requirements`, each task (with `idea_id`, `kind`, `profile`, `spec_json`, the ticket snapshot and its sha256 as `add-map` does today), `task_deps` from `after:`, the judge link from `judges:`, `task_covers`, and `plan_sha256` on the idea, and moves the idea to `in_progress`. Then fleetd writes `plan.verdict.json` with `valid`.
- Context boundary: ideas does not write the `tasks` table. `planned_tasks.rs` of task_execution is the port `AddPlannedTasks` that writes tasks, deps and covers inside the transaction that ideas opens.
- Tasks land in the internal run of the feature (MAP.md, Q5).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/plan_commit.rs`: `valid_plan_commits_each_task_in_1_transaction`, `a_failure_in_the_middle_commits_nothing` (a fault seam fails the 3rd task insert), `refused_plan_writes_the_verdict_and_stays_in_ticketing`, `committed_task_holds_the_ticket_snapshot`, `plan_sha256_is_on_the_idea`. They fail because the use case is empty.
2. Write `AddPlannedTasks` and `ValidatePlan`.
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
