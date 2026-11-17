# 30: The question flow: the sub-orchestrator of the feature owns each worker question, fleetd starts it until an answer is recorded, and 15 min with no answer escalates to you

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test question_flow && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/question_flow.rs`, `crates/horch-core/src/fleetd/ideas/application/questions.rs`, `crates/horch-core/src/fleetd/ideas/adapters/questions_store.rs`, `crates/horch-core/tests/question_flow.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (How a worker's question is resolved, steps 1 to 3 and 7); Requirements RR-12; Failure mode FM-35; Ledger (`questions`)).

## Goal

Each worker question has 1 owner, the sub-orchestrator of its feature, in place of the orchestrator of the run.

## Requirements

- RR-12
- FM-35
- The question flow with the sub-orchestrator as owner (replaces Q13 of fleetd-p2)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Today (Q13 of `docs/tickets/fleetd-p2/MAP.md`) `questions.feature_id` is the run id and the owner is the orchestrator of the run. Phase 6: `feature_id` is the feature (ticket 02 moved the rows), and `owner` is `idea-lead:<feature id>`.
- On a new question, fleetd rings the live process of the feature with the doorbell (a fixed line, FR-7). On each pass, while the question is `open`, fleetd asks ticket 28 to start the owner when it is not running.
- The time limit: `question_limit_s` (900 s) on the fleet clock, counted only while the owner process runs ("15 min with no answer from a running sub-orchestrator"). Past it: `escalated`, 1 needs-you item, and the notification. The wait for you has no limit; the task stays `blocked_on_user`.
- A change of the owner of a question is a transition with a cause, never a plain write.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `question_whose_owner_is_not_running_is_answered_after_fleetd_starts_it`, `question_escalates_after_15_min_and_operator_wait_has_no_limit`, `limit_counts_only_while_the_owner_runs`, `new_question_rings_the_owner_once`, `owner_is_the_sub_orchestrator_of_the_feature`. They fail because the flow is empty.
2. Write the flow. Change `questions.rs` and `questions_store.rs` of Phase 2 for the new owner.
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
