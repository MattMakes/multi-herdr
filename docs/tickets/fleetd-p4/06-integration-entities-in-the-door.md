# 06: The task branch, the bundle and the feature are entities of the 1 door: their state tables in transition(), and the rows store of integration

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime::domain::ledger && cargo test -p horch-core --test integration_door && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`, `crates/horch-core/src/fleetd/integration/adapters/sqlite.rs`, `crates/horch-core/tests/integration_door.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (Three rules: One door), "Landing a task branch on its feature branch" (the state table), "The bundle state machine", "The feature state machine".

## Goal

Each write of a `branches.landing_state`, a `bundles.state` and a `features.state` goes through `transition()`, with the allowed exits of the design and 1 event each. The rows store of integration reads and inserts the rows of the 6 integration tables.

## Requirements

- One door: the exits of the task branch table (`building`, `verifying`, `ready`, `in_bundle`, `landed`, `conflicted`, `gate_failed`, `abandoned`), of the bundle table (`building`, `gating`, `passed`, `landed`, `failed`, `dropped`) and of the feature table (`open`, `ready`, `merged`, `abandoned`; `open -> merged` is allowed). An illegal exit records an event and a needs-you item for the row, and the pass goes on (Phase 1 rule).
- OR-2: each change is an event with a cause. A state is never written without its entity: the event text names `task branch`, `bundle` or `feature`.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Phase 1 ticket 11 made `transition()`; Phase 2 ticket 04 added the task exits; Phase 3 ticket 03 added the worktree. Follow ticket 03 of fleetd-p3 for the shape.
- The rows store in `integration/adapters/sqlite.rs`: insert a feature row, a branch row, a bundle row with its members in 1 transaction, a gate run, a remote tip; read by key; set the facts that are not states (`seen_tip`, `seen_remote_tip`, `candidate_ref`, `candidate_tip`, `queue_seq`, `repair_attempts`, `push_*`, `pr_state`, `due_at`) with 1 event each. No other code writes these tables (an `arch_scan` rule exists from Phase 1 for foreign tables).
- Terminal states (`landed`, `abandoned`, `merged`, bundle `landed`, `failed`, `dropped`) have no exit.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/integration_door.rs`: `each_allowed_exit_of_the_3_tables_moves_and_records_1_event`, `illegal_exit_is_refused_and_opens_1_needs_you_item`, `terminal_states_have_no_exit`, `bundle_with_its_members_is_1_transaction`, `fact_writes_record_an_event_with_the_entity`.
2. Add the 3 entities and their tables to `domain/ledger.rs` and `adapters/sqlite.rs`, and write the rows store.
3. Run the Check. Commit the exact files.

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

Self-hosting: this phase changes `horch spawn`, the claim and the finish steps. The build fleet itself uses the installed `horch` for these. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).

## Done when

The Check passes. No code writes `landing_state`, `bundles.state` or `features.state` except through `transition()`.
