# 09: Platform: the count of processes and of open files of the user in the machine probe

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-platform`
**Own:** `crates/horch-platform/src/machine.rs`, `crates/horch-platform/src/mac/machine.rs`, `crates/horch-platform/src/linux/machine.rs`, `crates/horch-platform/src/windows/machine.rs`, `crates/horch-platform/src/unix/machine.rs`, `crates/horch-platform/tests/machine.rs`

Spec: `docs/specs/fleetd/design.md`, OR-5, PL-2 ("the machine probe (free disk, memory, load, the count of processes and of open files of the user)"), "Supervision, liveness and reconciliation" (Holds on dispatch are visible), `docs/specs/fleetd/preservation-06-wiring-and-platform.md` row S12.

## Goal

The machine probe of the platform also reads the count of processes of the user and the count of open files of the user, with the limit of each. The health guards of ticket 19 read them.

## Requirements

- **OR-5** (part): the probe gives the 5 facts of the health guards. On Windows each fact is `NotSupported` (row S12), and the guards report it.
- **PL-3** (part): a count that an OS cannot give is `NotSupported`.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Phase P ticket 09 (`docs/tickets/fleetd-pp/09-machine-probe-and-rest.md`) moved today's probe into `horch_platform::machine`: load, memory, disk, the process limit (`getrlimit`) and the host name. It has no count of processes or of open files of the user (MAP.md, Q15). Read the module doc of `crates/horch-platform/src/machine.rs` for the final names.
- Proposed names: `machine::user_process_count() -> Result<u64, NotSupported>`, `machine::user_open_file_count() -> Result<u64, NotSupported>`, `machine::open_file_limit() -> Result<u64, NotSupported>`.
- OS facts: macOS `proc_listallpids` and `proc_pidinfo(PROC_PIDTBSDINFO)` for the uid of each process, and `proc_pidinfo(PROC_PIDLISTFDS)` for the count of open files of each process of the user, or the system count `kern.num_files` when the per-process read is refused (say which in the module doc); Linux `/proc/<pid>/status` (`Uid:`) and the entries of `/proc/<pid>/fd`; Windows `NotSupported` (row S12). The open file limit is `getrlimit(RLIMIT_NOFILE)` on unix.
- A count must be fast: the pass reads it in part 1. A read of all processes in under 100 ms on this Mac is the bar. If it is slower, say so in the module doc and send NOTE.

## Steps

1. Write the tests first, in `crates/horch-platform/tests/machine.rs` (Phase P made the file; add tests, change none):
   - `user_process_count_is_at_least_1` (`#[cfg(unix)]`).
   - `user_open_file_count_grows_when_this_process_opens_files` (`#[cfg(unix)]`: open 20 temp files and compare).
   - `counts_are_not_supported_on_windows` (`#[cfg(windows)]`, run by CI).
2. Write the code. Each `unsafe` block has a `// SAFETY:` note.
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

## Done when

The Check passes on macOS. The 2 counts and the open file limit are known on macOS and Linux, and `NotSupported` on Windows.
