# 45: Retire the long-lived orchestrator conversation: horch fleet start launches no orchestrator, and fleetd no longer respawns one

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch --test p6_fleet_start && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch-core --lib taskledger::orchestrator && cargo test -p horch-e2e --test fleet_start`
**Own:** `crates/horch/src/cmd/recipes.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs`, `crates/horch-core/src/taskledger/orchestrator.rs`, `crates/horch/tests/p6_fleet_start.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 8 "Phase 6: the idea dispatcher and sub-orchestrators" ("Retire the long-lived orchestrator conversation"); Failure mode FM-16; Preservation analysis (rows S11 and V-9: CHANGED, dispatcher in code)).

## Goal

There is no long-lived orchestrator conversation. Sub-orchestrators per feature take its place.

## Requirements

- FM-16
- The end of the long-lived orchestrator conversation

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- `horch fleet start` (`start_orchestrator` in `crates/horch/src/cmd/recipes.rs`) no longer launches an orchestrator pane. It starts fleetd and herdr as today, and prints `Add an idea: horch idea add "..."`.
- The respawn duty (`orchestrator::check` called at `watchdog_duties.rs:748`) is removed with its guards (`RESPAWN_EVERY_S`, `RESPAWNS_PER_RUN`). `orchestrator::claim` and `launch_run` refuse with a line that names `horch idea add`.
- A live orchestrator of a run in flight at the update is not killed: it ends by itself, and it gets no more questions (ticket 50). fleetd does not stop it unless it is an orphan by I5 with a fact that proves it.
- Make `crates/horch-e2e/tests/fleet_start.rs` follow the new start (it is not in `Own`; if it needs a change, the orchestrator permits it in this ticket: send NOTE).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch/tests/p6_fleet_start.rs`: `fleet_start_launches_no_orchestrator`, `orchestrator_claim_is_refused_with_the_new_command`, and a unit test `no_respawn_duty_in_the_pass`. They fail because the orchestrator still starts.
2. Remove the launch, the respawn duty and the claim.
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
