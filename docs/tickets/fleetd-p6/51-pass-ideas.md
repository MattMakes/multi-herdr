# 51: Wire Phase 6 into fleetd, part 1: the dispatcher, the sub-orchestrators, the waits, the questions, the notes and answers, the replans, the roll-up, the plan commit, the needs-you mirror and the release duty

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_pass_p6a && cargo test -p horch-core --test fleetd_pass && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/scans.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/budget.rs`, `crates/horch/src/cmd/fleetd.rs`, `crates/horch-core/tests/fleetd_pass_p6a.rs`

Spec: `docs/specs/fleetd/design.md` (Supervision, liveness and reconciliation (the reconcile pass, Order inside 1 pass); Ledger (Three rules)).

## Goal

The pass runs each idea duty of Phase 6 in order, inside its work budget, and each effect after its commit.

## Requirements

- RR-4 and RR-7 for the idea duties
- PL-5 (the pass never waits for a job or a sub-orchestrator)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Follow tickets 46 and 47 of fleetd-p4 and 41 of fleetd-p5.
- Order: the release duty (first pass only); the ingest of idea folder files and notes; the answers; the question flow; the dispatcher; the plan commit; the replans; the roll-up; the sub-orchestrators (stops before launches); the waits; the needs-you mirror.
- Each duty has a row budget in `budget.rs` and a cursor in `pass_cursors`. The fact scan of the idea folders has the time budget of the fact scans of Phase 1.
- Effects after the commit: the launch and the stop of a sub-orchestrator, the write of `idea.md`, `approval.json`, `plan.verdict.json` and of the decisions file, a branch ref, a doorbell.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_pass_p6a.rs`: `pass_runs_the_idea_duties_in_order`, `crash_after_a_commit_completes_each_idea_effect_once`, `10000_idea_folders_keep_the_heartbeat_under_30_s` (fixture folders, the fleet clock), `pass_never_waits_for_a_sub_orchestrator`. They fail because the duties are not wired.
2. Wire the duties.
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
