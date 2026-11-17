# 22: Route the operator task and worker commands of horch ledger through requests: cancel, unblock, requeue, waive, fallback, reopen, dismiss, stop, reap, worker new, worker pid, worktree scratch and drop

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** full
**Check:** `cargo build -p horch && cargo test -p horch --test p5_route_ledger_task && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/taskledger.rs`, `crates/horch/tests/p5_route_ledger_task.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

The commands of `taskledger.rs` change no row themselves. Each change is 1 request to fleetd.

## Requirements

- RR-5 (each `horch` command that changes state sends a request; it opens the ledger read-only)
- FM-24

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- Each production write of the files of `Own` becomes 1 call of `channel_client::send` (ticket 15) with the kind of the MAP table "Request kinds". Each read becomes `taskledger::open_existing_read_only`. Code in `#[cfg(test)]` modules can still write to set up a test ledger.
- The command prints the result of the reply with the words of today. A refused reply prints `refused: <reason>` and exits 1. A queued request prints the line of ticket 15 and exits 0.
- The existing CLI tests of these commands must still pass. A test that starts no fleetd now needs one: use the test fleetd of `crates/horch-e2e/src/fleetd.rs`, or the in-process server of ticket 13, with temporary folders.
- When a write site has no kind in the MAP table, stop and send QUESTION with the site and a proposed kind.
- The kinds of these files: `task_cancel`, `task_unblock`, `task_requeue`, `ledger_waive`, `task_fallback`, `ledger_reopen`, `ledger_dismiss`, `worker_stop`, `worker_reap`, `worker_new`, `worker_pid`, `worktree_scratch`, `worktree_drop`.
- `taskledger.rs` is also the file of ticket 21 (before) and ticket 27 (after). The worker commands `progress`, `note`, `done`, `finish`, `handoff`, `claim` and `worktree new` are ticket 27.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch/tests/p5_route_ledger_task.rs`. Each test runs the development `horch` with temporary `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`, and a test fleetd. Each test asserts that the `requests` table has 1 row of the kind of the command, applied by fleetd, and the rows of today after it. Ticket 42 adds the seam that proves that the CLI process opens no writable connection. Tests:
   - `requeue_cancel_and_waive_send_their_kinds`
   - `worker_that_sets_horch_role_orchestrator_is_refused_by_fleetd`
   - `worker_that_removes_horch_role_is_refused_by_fleetd`
   - `stop_and_reap_send_their_kinds`
   - `worktree_drop_sends_worktree_drop`
   - `fleetd_down_queues_a_request_file`
2. Change each write site to `send`, and each read to read-only.
3. Run the existing tests of these files and keep them green.

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

- The Check passes, with the guard tests.
- The new tests pass.
- `grep -n "immediate(" crates/horch/src/cmd/taskledger.rs` finds no production line.
