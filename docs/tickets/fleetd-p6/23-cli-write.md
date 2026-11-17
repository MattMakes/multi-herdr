# 23: horch idea add, amend, approve, revise, park and cancel send their requests, and approve, add --direct and feature abandon refuse to run inside a fleet attempt

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch --test p6_idea_cli && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/idea/add.rs`, `crates/horch/src/cmd/idea/amend.rs`, `crates/horch/src/cmd/idea/approve.rs`, `crates/horch/src/cmd/idea/revise.rs`, `crates/horch/src/cmd/idea/park.rs`, `crates/horch/src/cmd/idea/cancel.rs`, `crates/horch/src/cmd/featurecmd.rs`, `crates/horch/tests/p6_idea_cli.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (Capturing an idea needs no AI; Quick approval stays with you, in code); Requirements SR-5, FR-10).

## Goal

The write commands of `horch idea` work end to end, and the first guard of the client refuses approve, `add --direct` and `feature abandon` inside a fleet attempt.

## Requirements

- FR-1 (through the CLI)
- SR-5 (the refusal inside a fleet attempt)
- FR-10

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Each command sends its request with the channel client of Phase 5 (`crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`), or writes a request file when fleetd is down, and prints the result. `horch idea add` prints the id.
- "Inside a fleet attempt": the process tree of the caller contains the recorded process of a live attempt (the process chain fact of ticket 26 of fleetd-p5), never an environment variable. The server checks of Phase 5 stay the wall; this is the first guard.
- `horch idea approve <id>` sends the sha256 of `assumptions.md` that it reads now, and prints the assumptions first (MAP.md, Q9).
- `crates/horch/src/cmd/featurecmd.rs` (ticket 05 of fleetd-p4, routed by ticket 25 of fleetd-p5) gets only the refusal inside a fleet attempt.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch/tests/p6_idea_cli.rs` with temporary folders: `idea_add_prints_the_id`, `idea_approve_inside_a_fleet_attempt_is_refused`, `idea_add_direct_inside_a_fleet_attempt_is_refused`, `feature_abandon_inside_a_fleet_attempt_is_refused`, `idea_cancel_needs_a_reason`. They fail because the commands are stubs.
2. Fill the 6 command files.
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
