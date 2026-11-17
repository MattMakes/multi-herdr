# 26: The health of a profile: a failed conformance run or a dialog marks it unhealthy, a passed run marks it healthy again, and dispatch skips an unhealthy profile

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test p7_profile_health && cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --lib fleetd::worker_profiles && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/domain/health.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/health.rs`, `crates/horch-core/src/fleetd/task_execution/application/stall.rs`, `crates/horch-core/src/fleetd/task_execution/domain/dispatch.rs`, `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/tests/p7_profile_health.rs`

Spec: `docs/specs/fleetd/design.md` ("Worker profiles and adapters" (The health of a profile); "Supervision, liveness and reconciliation" (rung 4); "Docs sync and adapter conformance" (step 4)).

## Goal

fleetd does not dispatch to a profile that a failed conformance run or a dialog marked unhealthy, and dispatches to it again after a passed run.

## Requirements

- FM-19: a CLI update that changes a flag or a default marks each profile of that CLI unhealthy before dispatch.
- FM-18 (part): a dialog (rung 4) marks the profile unhealthy until a conformance run passes (fleetd-p2 Q18).
- FR-8 (part): the unhealthy marking.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Choice (MAP.md Q10): no new table. The health of a profile is the last of its events `profile_unhealthy` and `profile_healthy` (ticket 02), with the profile name in `data_json`. No event means healthy. Each mark is a committed event row first.
- A failed `conformance` run of a CLI marks each profile with that `agent` unhealthy, with the failed checks as the cause. A passed run marks each unhealthy profile of that CLI healthy again. A mark that does not change the health writes no event.
- Rung 4 (`waiting_approval`) is in `crates/horch-core/src/fleetd/task_execution/application/stall.rs` (fleetd-p2 ticket 09): today it ends the attempt with `stalled` and 1 needs-you item. Add the mark of the profile.
- Dispatch (`crates/horch-core/src/fleetd/task_execution/domain/dispatch.rs` and `application/dispatch.rs`, fleetd-p4 ticket 08) skips a ready task whose profile is unhealthy. The fallback rule of the roster does not apply to an unhealthy profile: the design names it only for a usage limit and an outage. The task waits, and its hold has the cause "unhealthy profile <name>" (ticket 29 shows it).
- Contracts (Slice 7): `profile-marked-unhealthy`, `profile-marked-healthy`.

## Steps

1. Write the tests first in `crates/horch-core/tests/p7_profile_health.rs`: `unhealthy_profile_is_skipped_by_dispatch`, `profile_is_marked_healthy_again_when_conformance_passes`, `failed_conformance_marks_each_profile_of_the_cli`, `dialog_at_rung_4_marks_the_profile_unhealthy`, `contract_profile_marked_unhealthy`, `contract_profile_marked_healthy`, `same_health_writes_no_event`. They fail before the code.
2. Write the code. Run the Check and the guard tests. Commit the exact files.

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
