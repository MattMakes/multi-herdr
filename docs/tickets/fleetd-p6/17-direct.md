# 17: horch idea add --direct: your ticket makes a plan of 1 work task, plus its judge task for a judgment kind, that still passes the plan validator

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test idea_direct && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/direct.rs`, `crates/horch-core/tests/idea_direct.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (Everything is an idea, with a fast lane); Requirements FR-12; Features (a `--direct` idea)).

## Goal

Use case `AddDirectIdea`: a ticket that you wrote becomes an idea in `ticketing`, then 1 validated plan with 1 work task, or 2 tasks for a judgment kind.

## Requirements

- FR-12
- FR-10 (the ticket stands for your approval)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- fleetd copies the ticket into `ideas/I-NNNN/tickets/01-<name>.md` and writes `plan.yaml` itself (1 task, its profile from the Persona line, its check from the Check line). For a judgment kind fleetd adds the judge task: kind `judge`, `judges:` the work task, a profile of another provider from the roster (MAP.md, Q8).
- The idea goes from `captured` to `ticketing` under the rule of the feature (while another idea of the feature is in `clarifying`, `specifying` or `ticketing`, it waits in `captured`). Ticket 26 owns that rule; this use case only asks for the move.
- With no `--to`, the feature gets its branch name from the ticket title (ticket 15).
- A plan that the validator refuses: the idea goes to the needs-you list with the reason, and it stays in `ticketing`.
- The approval (MAP.md, Q6): fleetd writes `approval.json` with the sha256 of the copied ticket, and sets `approved_sha256` to it.
- The refusal inside a fleet attempt is in the client (ticket 23) and in the server checks of Phase 5. This use case does not decide it.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `intake_test_direct_idea_produces_1_validated_plan_with_1_work_task`, `direct_idea_of_a_judgment_kind_gets_its_judge_task`, `direct_plan_without_check_is_refused_and_goes_to_needs_you`, `direct_idea_waits_in_captured_while_another_idea_of_its_feature_is_ticketing`. They fail because the use case is empty.
2. Write `AddDirectIdea`.
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
