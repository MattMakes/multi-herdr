# 06: The request kinds of Phase 6: idea_add, idea_amend, idea_approve, idea_revise, idea_park and idea_cancel of class operator, and idea note as a file of class worker

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test requests_p6_kinds && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch-core --lib fleetd::fleet_runtime::domain::requests`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs`, `crates/horch-core/tests/requests_p6_kinds.rs`

Spec: `docs/specs/fleetd/design.md` (Architecture and roles (The command channel: Each request kind has a class)).

## Goal

The kind catalog of the request contract has each idea kind of the design with its class, and fleetd refuses `idea_note` on the channel and in a request file.

## Requirements

- SR-8 for the idea kinds
- FR-10 (the approval is an operator request)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- The catalog is in `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs` (ticket 08 of fleetd-p5). Its table is the "Request kinds" table of `docs/tickets/fleetd-p5/MAP.md`.
- `idea_add` carries `--direct` and `--to` in its payload (design: "`idea_add` (also `--direct` and `--to`)"). A `--direct` ticket file goes as an absolute path and its sha256 (Q15 of fleetd-p5).
- `idea note` is a worker kind: a file in the folder of its idea, written by the sub-orchestrator. It never uses the channel or the request folder.
- This ticket only adds kinds. Tickets 48 and 49 retire the kinds that Phase 6 replaces.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `each_idea_kind_is_operator`, `idea_note_on_the_channel_is_refused`, `idea_add_payload_has_direct_and_to`, `idea_add_direct_payload_has_the_sha256_of_the_ticket`. They fail because the kinds do not exist.
2. Add the kinds.
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
