# 27: The idea-lead persona: an interactive Opus 5.5 sub-orchestrator that runs the brief, spec-writer and ticket-writer steps for 1 idea, 1 phase for each process

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test idea_lead_persona && cargo test -p horch-core --test golden_prompts && cargo test -p horch-core --test harness_defaults && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `teammates/idea-lead.md`, `teammates/_base/fleet-idea-lead.md`, `crates/horch-core/tests/idea_lead_persona.rs`

Spec: `docs/specs/fleetd/design.md` (Architecture and roles (Sub-orchestrator); Ideas (A fresh process for every phase; The interview; How a worker's question is resolved); Worker profiles and adapters (Exec or interactive)).

## Goal

The roster has the persona `idea-lead`, and its briefing tells each phase what to read and what to write.

## Requirements

- The `idea-lead` persona (new work of Phase 6)
- FM-30 (research and interview first)
- FM-32 (its profile denies `horch idea approve` and the write of `approval.json`)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Frontmatter: `name: idea-lead`, `base: fleet-idea-lead`, `agent: claude`, `model: opus`, `effort: medium`, `fallbacks: [codex-sol]`, `mode: interactive`, and a sandbox of a worker (Phase 5: no token, no channel, no request folder). Its writable paths: its idea folder only.
- The briefing `teammates/_base/fleet-idea-lead.md` takes the steps of today from `teammates/spec-writer.md` and `teammates/ticket-writer.md` and states each phase: clarifying (read the code and docs, ask 1 question at a time, record each answer with `horch idea note`, write `assumptions.md` with `feature: <name>` and no `[open]` item); specifying (`spec.md`, `requirements.yaml`, each requirement cites `A<n>` or `Q<n>`); ticketing (tickets in format C1 and `plan.yaml`; run `horch idea show <id> --in-flight` first); supervising (answer the open questions of the feature with `horch idea note --question`, replan a failed task, fix or dismiss each HIGH finding). It answers open questions before it goes on with its phase.
- Project facts (`horch note --fact`) go to each sub-orchestrator as they go to each worker today.
- Do not edit `teammates/spec-writer.md` or `teammates/ticket-writer.md`: ticket 63 owns them.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write `crates/horch-core/tests/idea_lead_persona.rs` first: `idea_lead_loads_from_the_roster`, `idea_lead_is_interactive`, `idea_lead_sandbox_denies_the_operator_path`, `idea_lead_briefing_names_each_phase`. They fail because the persona does not exist.
2. Write the 2 files. Update a golden or oracle file only when the test names it, and say so in the summary.
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
