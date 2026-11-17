# 30: The wrapper (horch worker) writes heartbeat.json, exit.json and session.json, and no ledger row

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test wrapper_files && cargo test -p horch-core --lib taskledger::supervisor && cargo build -p horch && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/supervisor.rs`, `crates/horch/src/cmd/worker.rs`, `crates/horch-core/src/execution/store.rs`, `crates/horch-core/src/execution/lifecycle.rs`, `crates/horch-core/tests/wrapper_files.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

The supervisor `horch worker` reports the facts of its agent through files of the attempt folder.

## Requirements

- RR-5 (the wrapper writes files; fleetd is the only writer)
- RR-8 (a snapshot is replaced atomically and has a `seq`)
- pres 06 S11 (no Unix socket needed for a worker report)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- Until the fleet-run wrapper exists, `horch worker` is the wrapper (Phase 2 Q6; ticket 15 of fleetd-p4). Today it writes the ledger: `Store::open` and `immediate` in `crates/horch-core/src/taskledger/supervisor.rs` (lines 24 to 38: the agent start; line 217: the settle of the exit), the checkin thread of ticket 07 of fleetd-p2 (`supervisor.rs`, `execution/lifecycle.rs`), and the session records of `crates/horch-core/src/execution/store.rs` (`session_records`, line 622; the `UPDATE workers` of line 1315).
- The files, in the attempt folder `<fleet root>/tasks/<task>/attempts/<n>/` (the outbox is the agent's; these files are the wrapper's, Protocol rule 3):
   - `heartbeat.json`: the snapshot of the design example (task, attempt, pid, pgid, mux pane, proc state, agent state, last output, tokens, `seq`, `written_at`), every `heartbeat_every_s`; `seq` grows by 1 each time.
   - `exit.json`: the exit code and the time, 1 time.
   - `session.json`: the session id of the CLI and its harness, 1 time when the wrapper finds it.
- Each file: `name.tmp`, fsync, rename, mode 0600. The wrapper runs outside the sandbox of its agent and is a child of fleetd through the launcher.
- An operator spawn with no attempt (no attempt folder) keeps no session file; its session record is written by fleetd from the `spawn` request (ticket 19). If a write remains with no attempt and no request, stop and send QUESTION.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/wrapper_files.rs`: `wrapper_writes_heartbeat_with_a_growing_seq`, `wrapper_writes_exit_json_once`, `wrapper_writes_session_json_when_it_finds_the_session`, `wrapper_opens_no_writable_ledger` (the ledger file mode 0444 for the test), `heartbeat_is_replaced_atomically`.
2. Change each write of the 4 source files of `Own` to its file.

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
- The 5 tests pass.
- `grep -n "immediate(\|Store::open(" crates/horch-core/src/taskledger/supervisor.rs` finds no production line.
