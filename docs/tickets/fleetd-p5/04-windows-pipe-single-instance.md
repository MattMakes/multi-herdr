# 04: Platform: the Windows command channel has 1 pipe instance only, and a client tries again on PIPE_BUSY

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-platform && (rustup target list --installed | grep -q x86_64-pc-windows-msvc && cargo check -p horch-platform --target x86_64-pc-windows-msvc || true) && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-platform/src/windows/channel.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

No other process of the same user can make a second instance of the fleetd pipe and catch a client. fleetd holds the only instance for its whole life.

## Requirements

- SR-5 (the channel accepts only the user that owns the ledger)
- SR-8 (a worker cannot take the place of fleetd on the channel)
- REQUIRED item 1 of the ticket of this map (Open question Q1)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- Today `create_instance` in `crates/horch-platform/src/windows/channel.rs` calls `CreateNamedPipeW` with `PIPE_UNLIMITED_INSTANCES`. So a process of the same user can make a second instance with the same name, and a client can connect to it.
- Choice (a) of Open question Q1: `nMaxInstances` 1, and the first instance has `FILE_FLAG_FIRST_PIPE_INSTANCE`. fleetd never closes that handle while it lives: after each client it calls `DisconnectNamedPipe` and then `ConnectNamedPipe` again on the same handle. So there is no gap in which another process can make the instance. A client that gets `ERROR_PIPE_BUSY` waits with `WaitNamedPipeW` until its deadline, and then tries again (the code at line 239 does this today).
- Why (a) and not (b): (b) needs a worker sandbox that denies the creation of a pipe by name. No CLI sandbox on Windows gives that today (the Claude sandbox is not supported on Windows, design "What each OS provides"). (a) is a property of the server alone.
- The client check of the server process (ticket 05) is still necessary: it catches a pipe that another process made while fleetd was down.
- This Mac compiles only macOS code. CI on Windows proves this ticket. Keep the change small and read it 2 times.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first, in the `#[cfg(windows)]` test module of `windows/channel.rs`: `second_instance_of_the_channel_name_is_refused` (a second `CreateNamedPipeW` with the same name fails while the listener lives) and `client_waits_on_pipe_busy_and_connects_after_the_first_client_ends`. They fail on Windows CI today because the instances are unlimited.
2. Change `create_instance`: `nMaxInstances` 1 and `FILE_FLAG_FIRST_PIPE_INSTANCE` for the first instance. Change `accept` so that it reuses the 1 handle (`DisconnectNamedPipe`, then `ConnectNamedPipe`).
3. Keep the `PIPE_BUSY` wait of the client inside its deadline.
4. In the done summary, name each Windows risk: a client that holds the instance open blocks other clients until fleetd ends that connection (fleetd reads 1 line with a deadline, ticket 13); the CI job of Windows (`.github/workflows/ci.yml`, ticket 08 of fleetd-p0) is the only proof.

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
- `cargo test -p horch-platform` passes on this Mac.
- The 2 Windows tests are in the test module, and the Windows CI job runs them.
