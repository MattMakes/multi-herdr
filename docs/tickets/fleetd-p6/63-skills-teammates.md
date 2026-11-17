# 63: The skills and the teammate files of the old flow: the orchestrate skill, the orchestrator briefing, the spec-writer and the ticket-writer name the new flow

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** light
**Check:** `cargo test -p horch-core --test golden_prompts --test skills_catalog --test harness_defaults --test baseline_oracles && cargo test -p horch --test skills_cli && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `skills/orchestrate/`, `teammates/_base/fleet-orchestrator.md`, `teammates/orchestrator.md`, `teammates/orchestrator-codex.md`, `teammates/ticket-writer.md`, `teammates/spec-writer.md`, `teammates/_base/context-messages.md`, `teammates/README.md`, `crates/horch-core/tests/golden_prompts.rs`, `crates/horch-core/tests/golden/`, `crates/horch-core/tests/oracles/`, `crates/horch-core/tests/skills_catalog.rs`, `crates/horch/tests/oracles/`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 8 "Phase 6: the idea dispatcher and sub-orchestrators"; the table "Old flow and new" of `docs/tickets/fleetd-p6/MAP.md`).

## Goal

No skill or teammate file tells an agent to run a retired command. Each one names its new command.

## Requirements

- "What a user does today must still work, or have a clear new command" (the skills and teammates part)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- `skills/orchestrate/SKILL.md` steps 0 to 10 name `horch ledger run new`, `add-map`, `task`, `plan-workers`, `horch spawn`, `run gate` and `run close`. Rewrite it for you, the operator: `horch idea add`, `--direct`, `--to`, `horch ideas`, `horch idea show`, `approve`, `revise`, `amend`, `park`, `cancel`, `horch task requeue`, `horch task cancel`, `horch ledger stop`, `horch ledger fallback`, `horch ledger waive`, `horch feature abandon`. Keep its id `orchestrate`.
- `teammates/_base/fleet-orchestrator.md`, `teammates/orchestrator.md` and `orchestrator-codex.md`: the persona stays for a chat with you, with no fleet duties: it does not claim runs, spawn workers or get wakes (ticket 45). Write that in its briefing.
- `teammates/spec-writer.md` and `teammates/ticket-writer.md`: their steps are now phases of `idea-lead` (ticket 27). The ticket writer writes `plan.yaml` and tickets in format C1, and `add-map --dry-run` checks them.
- Update a golden or oracle file only for a text that this ticket changed, and say each one in the summary.
- Do not edit `CHANGELOG.md`.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Run the Check first and record which pinned files change.
2. Change the files. Run the Check.
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
- `git grep -n "plan-workers\|ledger run new" skills teammates` finds no instruction to run them.
