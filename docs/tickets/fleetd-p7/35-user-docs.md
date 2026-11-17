# 35: The docs for a new user: install, start fleetd, add an idea and read the status with the new commands, in the README, the docs, the skills and the teammate files

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `cargo test -p horch-core --test golden_prompts --test skills_catalog --test nfr && cargo test -p horch --test skills_cli && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `README.md`, `docs/README.md`, `docs/command-flow.md`, `docs/fleet-workflow.md`, `docs/skills-and-teams.md`, `docs/testing-and-gates.md`, `skills/orchestrate/`, `teammates/README.md`, `teammates/_base/fleet-worker.md`, `teammates/_base/fleet-orchestrator.md`, `teammates/ticket-writer.md`, `crates/horch-core/tests/golden/`

Spec: `docs/specs/fleetd/design.md` (REQUIRED item 5 of the map; Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode").

## Goal

A new user can install horch, start fleetd, add an idea and read the status with the new commands, from `README.md` alone, and no doc, skill or teammate file tells a user or an agent to use a retired command.

## Requirements

- REQUIRED item 5: the docs for a user.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Phase 6 updated the idea flow (fleetd-p6 tickets 63 and 64). This ticket is the final pass for the whole build: fleetd (Phase 1), worktrees (Phase 3), `horch result` and the landing (Phase 4), the request channel (Phase 5), ideas (Phase 6), docs sync, conformance, exec mode and 256000 (Phase 7).
- Find each file that names the old flow: `rg -l "add-map|ledger run new|horch ledger claim|ledger finish" README.md docs/*.md skills teammates`. Today that is `README.md`, `docs/command-flow.md`, `docs/fleet-workflow.md`, `skills/orchestrate/SKILL.md`, `teammates/ticket-writer.md`, `teammates/_base/fleet-worker.md`, `teammates/_base/fleet-orchestrator.md` and `teammates/_base/codex-execpolicy.md`. Do not change `codex-execpolicy.md`: it is sandbox config. Name each other file outside `Own` in your done summary.
- Do not edit `CHANGELOG.md`: it is generated. Do not edit `docs/specs/` or `docs/tickets/`.
- A change to a file under `teammates/_base/` changes a golden prompt. Update `crates/horch-core/tests/golden/` with the new text, and name each changed golden file in the commit message.
- Each command that a doc names must exist: run it with `--help` in a temporary folder setup (build rule 1), never on the live fleet.

## Steps

1. Write the README section "Start" in 4 steps: install, start fleetd, add an idea, read the status. Name the real commands.
2. Update each file of `Own` that names the old flow.
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
- The `rg` of the Context finds no old command in a file of `Own`, except where the text says that it is retired.
