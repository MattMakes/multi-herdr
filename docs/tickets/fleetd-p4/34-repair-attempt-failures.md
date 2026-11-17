# 34: A failed resolver or fix attempt adds 1 to repair_attempts and starts the next after the backoff; at max_attempts 1 needs-you item; a requeue of a done task resets it

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --test repair_attempts_limit && cargo test -p horch --bin horch -- taskcmd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/repair.rs`, `crates/horch-core/tests/repair_attempts_limit.rs`, `crates/horch/src/cmd/taskcmd.rs`

Spec: `docs/specs/fleetd/design.md`, "Landing a task branch" (rung 6 adds 1 to `repair_attempts`; `horch task requeue <task>` sets it to 0), "Leases, not claims" (the lease expiry of a resolver or fix attempt ends that attempt and does not move the task), FM-38, RR-6.

## Goal

A failure of a resolver or fix attempt (a stall, a crash, no result, a failed check, a lease expiry) does not move the `done` task. It adds 1 to `repair_attempts` of the branch, and fleetd starts the next attempt after the backoff. At `max_attempts` of the task, 1 needs-you item opens. `horch task requeue <task>` of a `done` task sets `repair_attempts` to 0 and starts 1 more attempt.

## Requirements

- A failed resolver attempt starts the next one, and at `max_attempts` it puts 1 item on the needs-you list (a test of the gate).
- RR-6: every retry is bounded.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Ticket 15 owns `domain/repair.rs` first (the repair pass rule); this ticket adds the repair attempt count in the same file.
- `horch task requeue` is `crates/horch/src/cmd/taskcmd.rs` (Phase 2 ticket 13).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `failed_resolver_attempt_starts_the_next_after_the_backoff`, `lease_expiry_of_a_fix_attempt_does_not_move_the_done_task`, `at_max_attempts_1_needs_you_item_opens`, `requeue_of_a_done_task_resets_repair_attempts_and_starts_1_more`.
2. Write the count, the backoff and the requeue.
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

The Check passes. Repair attempts are bounded, and the exit at the limit is 1 needs-you item.
