# 03: Platform concern: process identity and liveness

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-platform --test process && cargo clippy -p horch-platform --all-targets -- -D warnings`
**Own:** `crates/horch-platform/src/process.rs`, `crates/horch-platform/src/mac/process.rs`, `crates/horch-platform/src/linux/process.rs`, `crates/horch-platform/src/windows/process.rs`, `crates/horch-platform/src/unix/process.rs`, `crates/horch-platform/tests/process.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 (the concern "process identity and liveness (alive, start
time, parent, children)"), and `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
row "Process identity and liveness".

## Goal

`horch_platform::process` gives the start time of a process, its start as a `SystemTime`, whether
it is alive, whether it started by a time, its parent, and the members of a process group. The
behavior is the same as today on each OS.

## Requirements

- **PL-2** (part): the concern "process identity and liveness" is in the platform interface.
- **PL-6** (part): the gaps of today are explicit `NotSupported` values.

## Context

Today's code that this ticket copies (do not edit these files):

| Today | What it does | Platform function (proposed name) |
| --- | --- | --- |
| `crates/horch-core/src/procid.rs:185` `mod imp` (macOS): `start_time`, `started_at`, `started_by`, `group_members` | `proc_pidinfo` start time and group members | `process::start_time`, `started_at`, `started_by`, `group_members` |
| `procid.rs:233` `mod imp` (Linux): `start_time`, `parse_stat`, `stat_field`, `group_members`, `started_by`, `started_at` | `/proc/<pid>/stat` field 22 and btime from `/proc/stat` | the same 4 functions |
| `procid.rs:340` `mod imp` (Windows): the handle type, `running`, `start_time`, `started_at`, `filetime_to_system`, `started_by`, `group_members` | `OpenProcess` and `GetProcessTimes`; no group members | the same functions; `group_members` gives `NotSupported`; `filetime_to_system` becomes `pub`, because the test `filetime_counts_from_1601` of `procid.rs` calls it |
| `procid.rs:180` `windows_running` | Windows liveness | `process::running` |
| `crates/horch-core/src/fsx.rs:319` `pid_alive` (`:324` unix `kill(pid, 0)` with `EPERM` = alive, `:331` Windows, `:337` other) | liveness without a start time | `process::alive(pid)` |
| `crates/horch/src/cmd/spawn.rs:421`, `:443`, `:451` `parent_pid` | parent pid from `proc_pidinfo` (macOS) and `/proc` (Linux); `None` elsewhere | `process::parent(pid)`; Windows gives `NotSupported` (`spawn.rs:451`) |
| `procid.rs:450` `mod imp` for other targets | `None` everywhere | dropped: ticket 01 refuses other targets |

`procid.rs` keeps `raw`, `alive`, `is_same` and `stop_same` as its own logic. Ticket 10 switches it.

## Steps

1. Write the tests first, in `crates/horch-platform/tests/process.rs`. Each fails before the
   functions exist:
   - `start_time_of_this_process_is_stable`: 2 reads of `std::process::id()` give the same value.
   - `start_time_of_a_dead_pid_is_none`: start and wait for a child, then read its pid.
   - `alive_is_true_for_self_and_false_for_a_reaped_child`.
   - `started_by_now_is_true_for_self`.
   - `parent_of_a_child_is_this_process` (`#[cfg(unix)]`).
   - `parent_is_not_supported_on_windows` and `group_members_are_not_supported_on_windows`
     (`#[cfg(windows)]`, run by CI).
   - `start_time_parses_a_stat_line_with_a_name_with_spaces` (`#[cfg(target_os = "linux")]`):
     a `/proc/<pid>/stat` line whose name has spaces and `)` gives field 22.
2. Write `src/process.rs` (the interface: `pub fn` that call `crate::os::process`) and the 3 OS
   modules. Put code that macOS and Linux share (the `kill(pid, 0)` check) in `src/unix/process.rs`.
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
- Each row of the table has its platform function, and each gap gives `NotSupported`.
