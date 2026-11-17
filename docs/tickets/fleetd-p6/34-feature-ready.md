# 34: A feature is ready only when each of its ideas is integrating, cancelled or parked: the idea state replaces the Phase 4 stand-in

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test feature_states_p6 && cargo test -p horch-core --test feature_states && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/application/features.rs`, `crates/horch-core/src/fleetd/integration/domain/feature.rs`, `crates/horch-core/tests/feature_states_p6.rs`

Spec: `docs/specs/fleetd/design.md` (Features (The feature state machine); REQUIRED item 5).

## Goal

The exit of a feature from `open` to `ready` reads the states of its ideas.

## Requirements

- The rules of the feature level
- REQUIRED item 5 (Q20 of fleetd-p4)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Phase 4 used "each task of the run is `done` or `cancelled`" as the stand-in for the ideas of a feature (Q20 of `docs/tickets/fleetd-p4/MAP.md`, ticket 36 of fleetd-p4). Replace it: `open` -> `ready` needs each idea of the feature in `integrating`, `cancelled` or `parked`, and a passed full gate on the feature tip over the fetched primary tip.
- `ready` -> `open` when a new idea joins with `--to` (ticket 15 uses the door exit of ticket 04) or a tip moves (Phase 4).
- `open` -> `merged` by content when an idea is still in flight: this ticket only allows it in the domain rule; ticket 35 moves the ideas.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `feature_with_an_idea_in_progress_is_not_ready`, `feature_with_ideas_integrating_cancelled_or_parked_is_ready_after_the_full_gate`, `open_feature_merged_by_content_is_merged`. They fail because the stand-in is still in place.
2. Replace the stand-in. Make `feature_states` of Phase 4 follow it.
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
