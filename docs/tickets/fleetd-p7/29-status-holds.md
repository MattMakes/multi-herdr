# 29: horch status shows each hold on dispatch with its cause: a health floor, pacing, an unhealthy profile, a git job that holds the repository, a missing fast gate, and not_before

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch --test p7_status_cli && cargo test -p horch-core --lib taskledger::status && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/status.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs`, `crates/horch/tests/p7_status_cli.rs`

Spec: `docs/specs/fleetd/design.md` (OR-1; FM-45; "Holds on dispatch are visible").

## Goal

A ready task that does not run shows its cause in `horch status`, for each cause of the design, in 1 view with each idea, task, lease, worker and its last signal.

## Requirements

- OR-1: `horch status` shows every idea, task, lease, worker and its last signal in 1 view, and each hold on dispatch with its cause. fleetd records an event when a global hold starts and when it ends.
- FM-45: a ready task does not run, and the status says why.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The causes and their source: a health floor and pacing (`Hold::Floor`, `Hold::Pacing` in `crates/horch-core/src/fleetd/fleet_runtime/domain/health.rs`, Phase 1); the worktree disk hold (fleetd-p3 ticket 16); a missing fast gate (fleetd-p4 ticket 45); `not_before` (Phase 2); an unhealthy profile (ticket 26); a git job that holds the repository (MAP.md Q12).
- The hold "a git job that holds the repository": the rule of 1 git writer for each repository (fleetd-p3 Q12). A ready task whose `worktree_add` waits because another git writer job of its repository runs shows the cause "git job <kind> <id> holds <repo>". It is derived from the `jobs` rows; it is not a new rule.
- The events `hold_started` and `hold_ended` of `record_hold_changes` (`crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs:139`) exist for a global hold. Check that each global hold of the list records them.
- What the user reads keeps the words of today; each new line has a word that says what happened. `horch status` and `horch ideas` (Phase 6) give the 1 view.
- If a cause of the list has no source in the code at the time of this ticket, send QUESTION with the missing mechanism.

## Steps

1. Write the tests first in `crates/horch/tests/p7_status_cli.rs` (a temporary ledger): `ready_task_shows_the_cause_of_its_hold` with 1 case for each of the 6 causes, `status_shows_each_lease_and_the_last_signal_of_each_worker`, `global_hold_records_start_and_end_events`. They fail before the code.
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
