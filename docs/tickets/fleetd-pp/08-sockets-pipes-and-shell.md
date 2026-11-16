# 08: Platform concern: local sockets, pipes, shell and quoting

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-platform --test ipc --test shell && cargo clippy -p horch-platform --all-targets -- -D warnings`
**Own:** `crates/horch-platform/src/ipc.rs`, `crates/horch-platform/src/mac/ipc.rs`, `crates/horch-platform/src/linux/ipc.rs`, `crates/horch-platform/src/windows/ipc.rs`, `crates/horch-platform/src/unix/ipc.rs`, `crates/horch-platform/src/shell.rs`, `crates/horch-platform/src/mac/shell.rs`, `crates/horch-platform/src/linux/shell.rs`, `crates/horch-platform/src/windows/shell.rs`, `crates/horch-platform/src/unix/shell.rs`, `crates/horch-platform/tests/ipc.rs`, `crates/horch-platform/tests/shell.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 (the concerns "local sockets and pipes" and "shell and
quoting"), and `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6, rows
"Sockets and pipes" and "Shell and quoting".

## Goal

`horch_platform::ipc` gives a local stream socket and its listener with the deadline behavior of
today: connect, write and read a line each with the time that is left of 1 deadline. On Windows
each function gives `NotSupported`, as today. `horch_platform::shell` gives the dialect of a pane
shell, the absolute shell of a check, and the rule for the case of a path. The behavior is the
same as today on each OS. Phase 5 later deletes the worker sockets (row 06 S11) and then deletes
these functions with their callers.

## Requirements

- **PL-2** (part): the concerns "local sockets and pipes" and "shell and quoting" are in the
  platform interface.
- **PL-6** (part): the Windows gap of the sockets is an explicit `NotSupported` value.

## Context

Today's code that this ticket copies (do not edit these files):

| Today | What it does | Platform item (proposed name) |
| --- | --- | --- |
| `crates/horch-core/src/messaging/channel.rs:155` `connect_by`, `:184` `unix_address`, `:205` `new_nonblocking_socket`, `:227` `write_by`, `:242` `retry_after`, `:257` `read_line_by`, `:275` `wait_ready` | a non-blocking `AF_UNIX` connect with `poll` and 1 deadline | `ipc::LocalStream::connect_by(path, deadline)`, `write_by`, `read_line_by` |
| `channel.rs:500` `start_relay_after_bind`, `:547` `prepare_socket_path`, `:563` `clear_stale_socket`, `:575` `set_timeouts`, `:582` `accept_loop`, `:610` `handle_connection`, `:645` `read_socket_request`, `:657` `read_request_line`, `:10`, `:12`, `:6` | bind, mode of the socket file, accept, timeouts | `ipc::LocalListener::bind(path)`, `accept`, `set_nonblocking`; `LocalStream::set_timeouts`; the relay logic stays in `channel.rs` |
| `channel.rs:104` and `:482` (Windows) | refuse with "needs Unix sockets" | each `ipc` function gives `NotSupported` on Windows; `channel.rs` keeps its message |
| `crates/horch-core/src/messaging/report.rs:22` to `:56`, `:152` `read_request`, `:164` `serve_one`, `:177` `Server`, `:222` `Drop`, `:237` and `:244` `send` | the report socket of `horch_access: report` | the same `ipc` types; the protocol stays in `report.rs` |
| `crates/horch-core/src/execution/lifecycle.rs:1028`, `:1045` `start_report_server` | start the report server; refuse on Windows | the caller maps `NotSupported` to its message of today |
| `crates/horch-core/src/harness/prime.rs:349`, `:362` `listening` | whether a process listens on a socket | `ipc::LocalStream::connect` |
| `crates/horch-core/src/workspace/paneshell.rs:33` `PaneShell::host` | `PowerShell` on Windows, `Posix` elsewhere | `shell::PANE_SHELL_IS_POWERSHELL: bool` or an enum of the platform crate that `paneshell.rs` maps to `PaneShell` |
| `crates/horch/src/cmd/taskledger.rs:2518` | `/bin/sh` on unix, `sh` elsewhere | `shell::CHECK_SHELL: &str` |
| `crates/horch-core/src/runtime/process.rs:175` | lowercase a path on Windows (case-blind file system) | `shell::fold_path_case(&str) -> String`, or a const fact `PATHS_ARE_CASE_BLIND` |

Read `channel.rs` and `report.rs` in full before you design the 2 types. The types own the
`unsafe` code of today (`libc::socket`, `libc::connect`, `libc::poll`, `fcntl`), so `channel.rs`
and `report.rs` have no `libc` after ticket 13.

## Steps

1. Write the tests first, in `crates/horch-platform/tests/ipc.rs` and `tests/shell.rs`. Each
   fails before the items exist:
   - `connect_write_read_a_line_within_a_deadline` (`#[cfg(unix)]`): a listener in a temp folder
     echoes 1 line.
   - `connect_to_a_missing_socket_fails_at_once` (`#[cfg(unix)]`).
   - `read_line_by_times_out_at_the_deadline` (`#[cfg(unix)]`): a silent peer; the error kind is
     `TimedOut` and the time is close to the deadline.
   - `socket_path_too_long_is_refused` (`#[cfg(unix)]`): a path over the `sun_path` limit gives
     an error and no panic.
   - `local_sockets_are_not_supported_on_windows` (`#[cfg(windows)]`, run by CI).
   - `check_shell_is_absolute_off_windows` (`#[cfg(unix)]`).
   - `pane_shell_is_posix_off_windows` (`#[cfg(unix)]`) and `pane_shell_is_powershell_on_windows`
     (`#[cfg(windows)]`).
2. Write `src/ipc.rs`, `src/shell.rs` and the 3 OS modules of each. Shared unix code goes in
   `src/unix/ipc.rs` and `src/unix/shell.rs`.
3. Run the Check.

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

Rules of this phase (tickets 03 to 09):

- Copy the OS code of today with no change of behavior: the same system calls in the same order,
  the same timeouts, the same return values and the same error kinds. PL-6 forbids a change of
  behavior. Do not fix an old fault here; send it as a NOTE to the orchestrator.
- Do not edit a file of another crate. The callers keep their own copy until the move tickets 10
  to 15 switch them to this crate. A short time with 2 copies is expected.
- Give core code a fact or an action. Never add a function that returns the name of the OS for
  core code to branch on.
- Where an OS has no equivalent today, its module returns `NotSupported` (ticket 01), or a type
  that holds it. Core code decides what to do. The doc comment of the function names the row of
  `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6, that states the gap.
- A helper that a test of another crate calls today (for example a parser) becomes a `pub`
  function here, so the move ticket can keep that test with its name and its place.
- Each `unsafe` block has a `// SAFETY:` note. Keep the OS modules small, and write the Linux
  and Windows modules with care: this Mac compiles only the macOS module.
- Make the code compile for Linux and Windows as far as this Mac can check. When
  `rustup target list --installed` shows the target, also run
  `cargo check -p horch-platform --target x86_64-unknown-linux-gnu` and
  `cargo check -p horch-platform --target x86_64-pc-windows-msvc`. Today this Mac has Homebrew
  Rust with only `aarch64-apple-darwin`. CI on the draft pull request of `fleet/fleetd-build`
  compiles and tests Linux and Windows at each push. A CI fault on Linux or Windows after you land
  comes back as a fix task.

## Done when

- The Check passes on this Mac.
- Each row of the table has its platform item. The Windows gap of the sockets is
  `NotSupported`, and its doc names row "Sockets and pipes" and row S11.
