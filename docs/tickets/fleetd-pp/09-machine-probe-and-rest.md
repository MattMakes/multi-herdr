# 09: Platform concern: the machine probe and the small rest

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-platform --test machine --test rest && cargo clippy -p horch-platform --all-targets -- -D warnings`
**Own:** `crates/horch-platform/src/machine.rs`, `crates/horch-platform/src/mac/machine.rs`, `crates/horch-platform/src/linux/machine.rs`, `crates/horch-platform/src/windows/machine.rs`, `crates/horch-platform/src/unix/machine.rs`, `crates/horch-platform/src/rest.rs`, `crates/horch-platform/src/mac/rest.rs`, `crates/horch-platform/src/linux/rest.rs`, `crates/horch-platform/src/windows/rest.rs`, `crates/horch-platform/src/unix/rest.rs`, `crates/horch-platform/tests/machine.rs`, `crates/horch-platform/tests/rest.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 (the concerns "the machine probe (free disk, memory,
load, the count of processes and of open files of the user)" and "the small rest (for example
archive format, file identity, user PATH)"), OR-5 (a probe that returns unknown is reported), and
`docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6, row "Other", and row S12.

## Goal

`horch_platform::machine` reads load, memory, disk, the process limit and the host name, with the
same sources as today. On Windows each reading gives `NotSupported`, as today (row S12).
`horch_platform::rest` gives the archive of a release, the self-update restart, the user PATH of
Windows, the executable extensions, the install facts and the host tools line. The behavior is
the same as today on each OS.

## Requirements

- **PL-2** (part): the concerns "the machine probe" and "the small rest" are in the platform
  interface. File identity is in ticket 06. The sandbox host has no OS `cfg` today; see
  `MAP.md`, open question Q5.
- **PL-6** (part): the gaps of today are explicit `NotSupported` values.

## Context

Today's code that this ticket copies (do not edit these files):

| Today | What it does | Platform item (proposed name) |
| --- | --- | --- |
| `crates/horch-core/src/runtime/machine.rs:146` `mod sys` (unix): `load_average`, `host_name`, the memory reads, `:301` `disk`, `:311` `max_processes`; `:316` other; `:322` `mod sys` (not unix): all `None` | `getloadavg`, `gethostname`, `sysctl` and `vm_stat` (macOS), `/proc/meminfo` (Linux), `statvfs`, `getrlimit` | `machine::load_average`, `host_name`, `memory`, `disk(path)`, `max_processes`; Windows gives `NotSupported` |
| `machine.rs:351` `parse_vm_stat`, `:372` `parse_meminfo` (`cfg_attr(not(unix), allow(dead_code))`) | parsers that tests of `machine.rs` call | `pub fn machine::parse_vm_stat` and `parse_meminfo`, compiled on every OS so the tests keep their place |
| `crates/horch-core/src/fsx.rs:345` `host_name` (`:346`, `:358` empty on Windows) | the host in a lock owner | `machine::host_name`; the caller keeps the empty text on `NotSupported` |
| `crates/horch/src/cmd/selfupdate.rs:577` and `:585` `extract`, `extract_entry` (`tar.gz`), `:606` `extract` (`zip`) | unpack the release archive | `rest::extract_release(archive, dir)` and `rest::RELEASE_ARCHIVE_SUFFIX` |
| `selfupdate.rs:376`, `:381` in `restart` | `exec` on unix; on Windows start the new binary, wait, and exit with its code | `rest::replace_process(Command) -> io::Error` with the 2 behaviors of today |
| `crates/herdr-install/src/win.rs:165`, `:183` `ensure_user_path`, `:9` | the user PATH in the Windows registry (`winreg`) | `rest::ensure_user_path(dir)`; macOS and Linux give `NotSupported` |
| `crates/herdr-install/src/main.rs:307` `report_path` | Windows or unix advice | `rest::USER_PATH_IN_REGISTRY: bool`, or the 2 report functions move into `herdr-install` behind that fact |
| `crates/herdr-install/src/place.rs:21` `exe_name` | `herdr.exe` or `herdr` | build it from `std::env::consts::EXE_SUFFIX`, or `rest::exe_name(stem)` |
| `crates/horch-core/src/runtime/process.rs:16` `path_extensions` | `PATHEXT` entries on Windows, none elsewhere | `rest::path_extensions(pathext: Option<&str>) -> Vec<String>` with the same rule (the caller passes the variable) |
| `crates/horch/src/cmd/install.rs:91` `remove_stale_old`, `:113` `add_to_path_help`, `:155` `install_launchers`, `:286` `swap_in(.., cfg!(windows), ..)` | Windows `.old` files, the PATH advice, unix-only launchers, the swap rule of a running exe | 1 fact or 1 function for each, for example `rest::RUNNING_EXE_IS_LOCKED: bool`, `rest::path_help(dir)`, `rest::SHELL_LAUNCHERS: bool` |
| `crates/horch-core/src/project_notes.rs:99`, `:101` `host_line` | the host tools line (BSD, GNU, other) | `rest::HOST_TOOLS: &str` |
| `crates/horch-core/src/harness/prime.rs:451` `may_stop` | on Windows the Prime status gives no start times | `rest::STATUS_HAS_START_TIMES: bool`, or a better name for the fact |

## Steps

1. Write the tests first, in `crates/horch-platform/tests/machine.rs` and `tests/rest.rs`. Each
   fails before the items exist:
   - `load_memory_disk_and_limit_are_known` (`#[cfg(unix)]`).
   - `probe_is_not_supported_on_windows` (`#[cfg(windows)]`, run by CI): each `machine` function
     gives `NotSupported`.
   - `parse_vm_stat_and_parse_meminfo_read_a_sample`: on every OS.
   - `extract_release_unpacks_the_archive_of_this_os`: build a small archive of the host format in
     a temp folder and unpack it.
   - `path_extensions_are_empty_off_windows` (`#[cfg(unix)]`) and
     `path_extensions_follow_pathext_on_windows` (`#[cfg(windows)]`).
   - `ensure_user_path_is_not_supported_off_windows` (`#[cfg(unix)]`).
   - `host_tools_line_matches_today` on the host OS.
2. Write `src/machine.rs`, `src/rest.rs` and the 3 OS modules of each. Shared unix code goes in
   `src/unix/machine.rs` and `src/unix/rest.rs`.
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
- Each row of the table has its platform item. Each Windows gap is `NotSupported`, and the doc of
  the probe names row S12.
