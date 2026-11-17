# 05: Platform: a client of the channel gets the process of the server, on each OS

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-platform --test channel && cargo test -p horch-platform && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-platform/src/channel.rs`, `crates/horch-platform/src/mac/channel.rs`, `crates/horch-platform/src/unix/channel.rs`, `crates/horch-platform/src/windows/channel.rs`, `crates/horch-platform/tests/channel.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

`Stream::server_pid()` gives the PID of the process at the other end of a client connection, so that a client can compare it with the `fleetd` row.

## Requirements

- SR-5
- SR-8
- REQUIRED item 2 of the ticket of this map: a client checks that the peer is fleetd (Open question Q8)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A process of the same user can remove the socket file in the 0700 folder and bind its own. The client then talks to that process. The client check: the PID of the server and its start time equal `pid` and `pid_start` of the `fleetd` row (ticket 15 makes the compare).
- The facts: macOS `LOCAL_PEERPID` on the client socket; Linux `SO_PEERCRED` on the client socket; Windows `GetNamedPipeServerProcessId`. The start time comes from the process facts of the platform (`crates/horch-platform/src/process.rs`).
- A fact that an OS cannot give is `NotSupported`, never a guess (PL-3). A guard whose fact is unknown does not hold: the client refuses the server.
- The module doc of `channel.rs` lists the facts of each OS. Update it.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-platform/tests/channel.rs`: `client_gets_the_pid_of_the_server` (a listener in this process: the client sees `std::process::id()`), and `client_gets_the_pid_of_a_server_in_a_child_process` (the server runs in a child, and the client sees the PID of the child).
2. Add `pub fn server_pid(&self) -> Result<u32, NotSupported>` to `Stream`, with the code of each OS.
3. Build for Windows when the target is installed. CI on Windows runs the tests.

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
- The tests pass on this Mac.
- The doc of `channel.rs` names the fact of each OS.
