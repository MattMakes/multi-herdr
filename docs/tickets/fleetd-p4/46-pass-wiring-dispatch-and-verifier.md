# 46: The pass of Phase 4, part 1: dispatch, launch, the outbox scan and ingest, the verifier, the review steps and the repair attempts in the reconcile pass, in the order of the design

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_pass_p4a && cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/scans.rs`, `crates/horch/src/cmd/fleetd.rs`, `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch-core/tests/fleetd_pass_p4a.rs`

Spec: `docs/specs/fleetd/design.md`, "The reconcile pass" (3 parts), "Order inside 1 pass", "A pass has a work budget", PL-5 (a pass never waits for a job), RR-4, FM-11.

## Goal

Each pass of fleetd runs the Phase 4 work of task execution: part 1 scans the outboxes (with its cursor and time budget); part 2 ingests results and questions before it renews or expires leases, observes the check jobs and decides the verifier and review steps, decides dispatch and repair attempts, and writes the intended rows; part 3 writes task files and launches bound attempts after the commit. `horch ledger watch --once` runs the same pass.

## Requirements

- Order: `ingest` and `finish_exited` run before `renew_or_expire_lease`.
- PL-5: no step waits for a job, a gate or a git network call.
- Each new row kind has its cursor in `pass_cursors` (`dispatch`, `outboxes`).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Phase 1 ticket 21, Phase 2 ticket 14 and Phase 3 ticket 10 wired the pass; follow them.
- Ticket 47 wires the integration part after this ticket.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_pass_p4a.rs`: `pass_binds_a_ready_task_and_launches_it_after_the_commit`, `result_in_the_pass_wins_over_the_lease_expiry`, `pass_queues_the_check_and_does_not_wait`, `outbox_scan_continues_at_its_cursor_after_a_restart`, `watch_once_runs_the_phase4_pass`.
2. Wire the pass.
3. This ticket moves a ref, lands, pushes, deletes a branch or a ref, or changes who runs the finish steps. It has a crash test: a fault point before the commit of the intended row, and after the commit and before the effect. In each case the next pass completes or fails the effect 1 time, and no tip and no work is lost.
4. Run the Check. Commit the exact files.

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

The Check passes. A ready task goes through bind, launch, result, check, review steps and `done` by passes alone.
