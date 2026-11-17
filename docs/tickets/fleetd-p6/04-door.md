# 04: The idea is an entity of the 1 door: its 10 states and exits in transition(), its store, and the feature exits open to merged and ready to open

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test ideas_door && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch-core --lib fleetd::fleet_runtime`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`, `crates/horch-core/src/fleetd/ideas/domain/idea.rs`, `crates/horch-core/src/fleetd/ideas/adapters/ideas_store.rs`, `crates/horch-core/tests/ideas_door.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (The idea state machine); Features (The feature state machine); Ledger (Three rules that make the ledger trustworthy, rule 1)).

## Goal

Each change of an idea state goes through `transition()`, with the exits of the design table and an event with a cause. The feature table of the door has the exits `open` to `merged` and `ready` to `open`.

## Requirements

- OR-2 for the idea entity (each state change is an event with a cause)
- The exit of feature `open` to `merged` (in the door; ticket 34 and ticket 35 use it)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- The entity table is in `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs` (`Entity`). Ticket 06 of fleetd-p4 added the task branch, the bundle and the feature in the same way. Follow it.
- The exits of an idea: `captured` -> `clarifying`, `ticketing` (for `--direct`), `parked`, `cancelled`; `clarifying` -> `awaiting_approval`, `parked`, `cancelled`; `awaiting_approval` -> `specifying`, `clarifying` (a revise), `parked`, `cancelled`; `specifying` -> `ticketing`, `awaiting_approval` (an amend after approval, MAP.md Q9), `parked`, `cancelled`; `ticketing` -> `in_progress`, `parked`, `cancelled`; `in_progress` -> `integrating`, `parked`, `cancelled`; `integrating` -> `merged`, `in_progress` (a new task from a replan), `parked`, `cancelled`; `parked` -> the state it left (a resume, MAP.md Q10), `cancelled`; `merged` and `cancelled` are terminal.
- Each event of an idea names the entity: `idea I-0012 clarifying -> awaiting_approval` (design: A state name is never written without its entity).
- `ideas_store.rs` reads and writes the rows of `ideas` and `requirements`. It writes the state only through the door.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/ideas_door.rs`: `each_exit_of_the_design_is_allowed`, `merged_and_cancelled_are_terminal`, `an_illegal_idea_transition_is_refused_and_goes_to_needs_you`, `feature_open_to_merged_is_allowed`, `feature_ready_to_open_is_allowed`, `idea_event_names_the_entity`. They fail because the idea is not an entity.
2. Add the entity, its table, the store and the feature exits.
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
- No code writes `ideas.state` outside the door (the test greps for `UPDATE ideas SET state`).
