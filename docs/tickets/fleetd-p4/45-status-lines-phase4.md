# 45: horch status shows the Phase 4 facts: the hold missing fast gate, a persona with no check sandbox, the bundle ref count, the rescue classes, the push state and a stale remote

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib taskledger::status && cargo test -p horch --test p4_status_cli && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/status.rs`, `crates/horch-core/src/fleetd/integration/application/queries.rs`, `crates/horch/tests/p4_status_cli.rs`

Spec: `docs/specs/fleetd/design.md`, OR-1, "fleetd owns the candidate refs" (`horch status` shows the count of bundle refs), "Rescue refs have 2 classes" (`horch status` shows the count and the bytes of each class), "Remote git state is level-triggered" (since when the remote tip is stale), FM-45.

## Goal

`horch status` (today `horch ledger status`, Phase 3 Q13) shows, in the words of today plus 1 new line for each new fact: each hold with its cause (also `missing fast gate` and each persona with no check sandbox), the count of bundle refs, the count and bytes of `unique` and `superseded` rescue refs, the push state of each feature, and since when a remote tip is stale. `--json` has the same facts.

## Requirements

- OR-1 (Phase 4 facts). Answer of the orchestrator (Q-A, rule 2): a clear line for a persona with no check sandbox.
- A state is never written without its entity: "task branch ready", "feature open", "bundle gating".

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The reads are in `integration/application/queries.rs`; `status.rs` prints them. Keep each line of today.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch/tests/p4_status_cli.rs` (a temporary ledger): `status_shows_missing_fast_gate_hold`, `status_shows_persona_without_check_sandbox`, `status_shows_bundle_ref_count_and_rescue_classes`, `status_shows_stale_remote_since`, `status_json_has_the_phase4_facts`.
2. Write the queries and the lines.
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

The Check passes. Each Phase 4 hold and fact has 1 line in `horch status`.
