# 41: The judge pair in the verifier: the judged task waits in verifying, both reach done with no plan change, a CRITICAL item starts a fix round, and after 2 fix rounds both are escalated

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test judge_pair && cargo test -p horch-core --test verifier && cargo test -p horch-core --test verifier_structural && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/judge_pair.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/tests/judge_pair.rs`

Spec: `docs/specs/fleetd/design.md` (Architecture and roles (The judge pair, steps 1 to 7; the rank table); Requirements RR-3; Failure modes FM-9, FM-33).

## Goal

A task of a judgment kind reaches `done` only after its paired judge, with no change of the plan. FM-9 is green with this ticket and ticket 42.

## Requirements

- RR-3 (the judgment part)
- FM-9
- FM-33
- The judge pair with its fix rounds (fixed contract D4: 2 rounds)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Phase 4 (ticket 21 of fleetd-p4): a judgment task passes on its structural check and today's finish steps until Phase 6. Remove that stand-in.
- Steps of the design: the judged task stays `verifying` (branch `building`) after its structural check; the judge task runs; no open CRITICAL -> the judge task `done`, then the judged task `done`, then its branch `ready`, then the judge branch `ready`, which lands after the judged branch (`queue_seq` order).
- Open CRITICAL: the judge attempt ends `critical_open`; `fix_rounds` + 1 on the judged task; the judged task `verifying` -> `ready` for its fix attempt (a fresh session, the same worktree, the verdict file in `START.md`), not counted in `max_attempts`; the judge task `verifying` -> `pending`. When the fix attempt passes its structural check, ticket 42 moves the judge branch, and the judge runs again in a fresh session, scoped to the earlier CRITICAL items and the change. A second judge cannot raise a new MEDIUM or LOW item: the verifier drops them and logs it.
- After 2 fix rounds with an open CRITICAL item: both tasks -> `escalated`, 1 needs-you item. Neither leaves `done`.
- HIGH items never block. Each HIGH item goes to the decisions file of the feature and rings the sub-orchestrator; it fixes it with a replan or dismisses it with `horch idea note --dismiss <finding> <reason>` (ticket 25; MAP.md, Q18).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `judged_task_stays_verifying_until_its_judge_task_is_done`, `rr3_judgment_task_and_its_judge_reach_done_with_no_plan_change`, `critical_item_gives_1_fix_round_not_counted_in_attempts`, `third_critical_round_escalates_to_the_operator`, `second_judge_new_medium_items_are_dropped`, `high_item_does_not_block`. They fail because the stand-in of Phase 4 is still in place.
2. Write the use case and change the verifier.
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
