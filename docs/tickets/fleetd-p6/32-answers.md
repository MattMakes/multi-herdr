# 32: The 4 answer cases of a worker question: covered with a checked citation, revise to you, replan, or an own decision, and the decision log of the feature

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test question_answers && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/answers.rs`, `crates/horch-core/src/fleetd/ideas/domain/answer.rs`, `crates/horch-core/tests/question_answers.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (How a worker's question is resolved, steps 4 to 6); decision D13).

## Goal

Use case `AnswerQuestion`: an answer note closes its question by 1 of the 4 cases of the design, and the answer reaches the worker and each later task of the feature.

## Requirements

- RR-12 (the answer is recorded)
- D13 (an own decision is a reversible assumption)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- `covered`: the note has `cite: <file>:<line>` in `assumptions.md` or `spec.md` of an idea of the feature. fleetd checks that the line exists; a missing citation leaves the question open and rings the owner again.
- `revise`: the answer changes an approved assumption. The idea goes to you as a revise (ticket 18, `awaiting_approval`). Only the tasks that the question names pause; the question stays open until your approval.
- `replan`: ticket 31 runs with the note text.
- `own_decision`: fleetd appends `N. [assumed, own decision] <text> (Q<n>)` to `assumptions.md` of the idea. It does not need a new approval; you can reverse it with `horch idea revise`.
- Each answer goes to the decisions file of the feature, `<fleet root>/features/F-NNNN/decisions.md` (MAP.md, Q15). Ticket 49 names it in `START.md` of each later task.
- Then the task leaves `blocked_on_user` to `ready`, and the resume of Phases 2 and 4 runs (resume the session with the answer, or a fresh worker from the question, the answer and the committed work).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `covered_answer_with_a_missing_citation_is_refused`, `covered_answer_moves_the_task_to_ready`, `revise_answer_pauses_only_the_named_tasks`, `own_decision_is_appended_as_an_assumption`, `each_answer_goes_to_the_decisions_file`. They fail because the use case is empty.
2. Write `AnswerQuestion`.
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
