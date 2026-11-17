# 20: horch idea park and cancel: worktrees are rescued first, a cancel cancels each task that is not terminal, and an abandoned feature cancels its ideas

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test idea_park_cancel && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/park_cancel.rs`, `crates/horch-core/tests/idea_park_cancel.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (The idea state machine, `parked` and `cancelled`); Features (`abandoned`); Supervision (A cancel)).

## Goal

Use cases `ParkIdea` and `CancelIdea`. No work is lost on a park or a cancel.

## Requirements

- FM-39 (a cancelled task does not block; its branch is rescued)
- A cancel of an idea follows A cancel for each task (A34)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- `CancelIdea`: each task of the idea that is not terminal gets the cancel of Phase 2 (`crates/horch-core/src/fleetd/task_execution/application/cancel.rs`): the stop of a live attempt first, then `cancelled`, then a rescue ref and the branch `abandoned`. The idea moves to `cancelled` when each of its tasks is terminal.
- `ParkIdea`: the idea stops its sub-orchestrator process (ticket 28 does the stop) and moves to `parked` with a reason. Its running tasks go on; no new task of it is dispatched (MAP.md, Q10). A worktree of the idea that is not `active` is rescued first.
- Policy: the event `feature-abandoned` cancels each idea of the feature that is not `merged` (A34). The handler is in this file.
- A resume of a parked idea: `horch idea park <id>` again is refused; `horch idea approve` or an answer resumes it to the state it left (MAP.md, Q10).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `cancel_of_an_idea_cancels_each_task_that_is_not_terminal`, `cancel_stops_a_live_attempt_before_the_task_is_cancelled`, `abandoned_feature_cancels_each_idea_that_is_not_merged`, `park_keeps_running_tasks_and_stops_dispatch`, `park_rescues_each_worktree_that_is_not_active`. They fail because the use cases are empty.
2. Write the use cases and the handler.
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
