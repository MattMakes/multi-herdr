# 04: Platform concern: signals and stop

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-platform --test signal && cargo clippy -p horch-platform --all-targets -- -D warnings`
**Own:** `crates/horch-platform/src/signal.rs`, `crates/horch-platform/src/mac/signal.rs`, `crates/horch-platform/src/linux/signal.rs`, `crates/horch-platform/src/windows/signal.rs`, `crates/horch-platform/src/unix/signal.rs`, `crates/horch-platform/tests/signal.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 (the concern "signals and stop"), and
`docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6, row "Signals and stop".

## Goal

`horch_platform::signal` stops a process only while it has its recorded start time, sends a polite
stop where the OS has one, and installs the signal handlers of today. The behavior is the same as
today on each OS. A wrong stop can kill a process that is not ours, so this ticket has the hardening
persona and a full review.

## Requirements

- **PL-2** (part): the concern "signals and stop" is in the platform interface.
- **PL-6** (part): the gaps of today are explicit `NotSupported` values.

## Context

Today's code that this ticket copies (do not edit these files):

| Today | What it does | Platform function (proposed name) |
| --- | --- | --- |
| `crates/horch-core/src/procid.rs:118` `signal_same`, `:125` `signal_os`, `:127` Linux `imp::pidfd_signal` | send a signal only while the start time matches; on Linux through `pidfd_send_signal` | `signal::signal_same(pid, started, signal)` |
| `procid.rs:156` `stop_same` (`:157` unix SIGTERM or SIGKILL; `:165` Windows `imp::terminate_same`, `TerminateProcess` exit 1 for both `Stop::Term` and `Stop::Kill`) | stop the recorded process | `signal::stop_same(pid, started, how)`, with the `Stop` enum moved here |
| `crates/horch-core/src/taskledger/watchdog.rs:673` and `:702` `stop` | SIGTERM a stale watchdog and wait; Windows refuses with a message | `signal::terminate_politely(pid, started)`; Windows gives `NotSupported` |
| `crates/horch-core/src/telemetry/lock.rs:65` and `:96` `stop` | the same for a stale collector | the same function |
| `crates/horch-core/src/harness/prime.rs:495` and `:505` `terminate` | SIGTERM by start time; Windows runs `taskkill` | `signal::terminate_politely`; core keeps the `taskkill` fallback on `NotSupported` |
| `crates/horch-core/src/taskledger/finish/land.rs:183` and `:224` `mod signals` | on SIGINT, SIGTERM or SIGHUP, SIGKILL the group of the running check, then raise the signal again with the default action; a no-op on Windows | `signal::kill_group_on_termination(&'static AtomicI32)` or an equal API; Windows gives `NotSupported` |
| `crates/horch/src/cmd/taskledger_watch.rs:30` `on_term` and `:39` | SIGTERM, SIGINT, SIGHUP set a stop flag | `signal::on_termination_set(&'static AtomicBool)`; Windows gives `NotSupported` |
| `crates/horch/src/cmd/telemetry/mod.rs:46` `on_signal` and `:54` | the same for the collector | the same function |
| `crates/horch-core/src/harness/prime.rs:642` and `:652` `outlive_hangup` | ignore SIGHUP with an empty handler | `signal::outlive_hangup()`; Windows gives `NotSupported` |

Read each handler before you write its platform version. Each handler body must stay
async-signal-safe.

## Steps

1. Write the tests first, in `crates/horch-platform/tests/signal.rs`. Each fails before the
   functions exist:
   - `signal_same_signals_only_the_recorded_process` (`#[cfg(unix)]`): a wrong start time sends
     nothing; the right one sends.
   - `stop_same_stops_only_the_recorded_process`: a child with a wrong start time lives; with the
     right one it ends.
   - `terminate_politely_ends_a_child_that_handles_sigterm` (`#[cfg(unix)]`).
   - `terminate_politely_is_not_supported_on_windows` (`#[cfg(windows)]`, run by CI).
   - `termination_flag_is_set_by_sigterm` (`#[cfg(unix)]`): start a child `horch-platform` test
     helper or use `libc::raise` in a test process that installs the flag, and check the flag.
   - `handlers_are_not_supported_on_windows` (`#[cfg(windows)]`).
2. Write `src/signal.rs` and the 3 OS modules. Shared unix code goes in `src/unix/signal.rs`. Use
   `crate::process` of ticket 03 (a blocker of this ticket) for each start time.
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
- Each row of the table has its platform function. Each Windows gap gives `NotSupported`.
- No signal is sent to a process whose start time does not match.
