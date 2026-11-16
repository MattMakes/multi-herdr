# 08: Platform: the transport of the command channel, with the peer user and the peer process

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-platform`
**Own:** `crates/horch-platform/src/channel.rs`, `crates/horch-platform/src/mac/channel.rs`, `crates/horch-platform/src/linux/channel.rs`, `crates/horch-platform/src/windows/channel.rs`, `crates/horch-platform/src/unix/channel.rs`, `crates/horch-platform/tests/channel.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 ("the transport of the fleetd command channel with its peer-user check"), "Architecture and roles" (The command channel: "Only you can send a command", "A second check on the live channel"; table "What each OS provides": Command channel transport).

## Goal

The platform gives the transport of the fleetd command channel: a local listener and stream (a Unix socket on macOS and Linux, a named pipe on Windows), and for each accepted connection the user and the process of the peer. Phase 5 uses it for the channel. Phase 1 adds it to the interface, as the design asks.

## Requirements

- **PL-2** (part): the concern "the transport of the fleetd command channel with its peer-user check".
- **PL-3** (part): a fact that an OS cannot give is `NotSupported`, never a guess.
- SR-5 and SR-8 become green in Phase 5. This ticket gives their platform facts only.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Phase P ticket 08 (`docs/tickets/fleetd-pp/08-sockets-pipes-and-shell.md`) moved the Unix socket code of today into `horch_platform::ipc` (`LocalStream`, `LocalListener`). Read the module doc of `crates/horch-platform/src/ipc.rs`. Build on it for macOS and Linux. Do not change `ipc.rs`: it is not in `Own`. If the channel needs a change of `ipc.rs`, send QUESTION.
- Proposed names: `channel::Listener::bind(name: &Path) -> io::Result<Listener>` (a socket file with mode 0600 in a folder of mode 0700 on unix; a named pipe `\\.\pipe\<name>` with an access rule for the owner only on Windows), `Listener::accept() -> io::Result<(Stream, Peer)>`, `Stream::connect(name, deadline)`, `Peer { uid: Result<u32, NotSupported>, pid: Result<u32, NotSupported> }` (on Windows a SID text instead of `uid`).
- OS facts: macOS `getpeereid` for the user and `LOCAL_PEERPID` (`getsockopt` level `SOL_LOCAL`) for the process; Linux `SO_PEERCRED`; Windows `GetNamedPipeClientProcessId` and the token of that process for the user.

## Steps

1. Write the tests first, in `crates/horch-platform/tests/channel.rs`. Each fails before the code exists:
   - `connect_and_read_a_line` (a listener in a temp folder, 1 client).
   - `peer_is_this_user_and_this_process` (the client is this test process: `uid` is the uid of the test, `pid` is `std::process::id()`).
   - `peer_pid_of_a_child_client_is_the_child` (the client is a child process; on unix spawn the test binary with an env flag).
   - `socket_file_has_mode_0600` (`#[cfg(unix)]`).
   - `#[cfg(windows)]` versions, run by CI.
2. Write the code. Each `unsafe` block has a `// SAFETY:` note.
3. Run the Check. A Linux or Windows result comes from CI.

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

The Check passes on macOS. The peer user and the peer process are read from the OS, and a fact that an OS cannot give is `NotSupported`.
