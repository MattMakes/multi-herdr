# 02: The Phase 1 concern files of horch-platform

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-platform && cargo build --workspace --locked`
**Own:** `crates/horch-platform/Cargo.toml`, `crates/horch-platform/src/lib.rs`, `crates/horch-platform/src/mac.rs`, `crates/horch-platform/src/linux.rs`, `crates/horch-platform/src/windows.rs`, `crates/horch-platform/src/unix.rs`, `crates/horch-platform/src/lock.rs`, `crates/horch-platform/src/awake.rs`, `crates/horch-platform/src/service.rs`, `crates/horch-platform/src/watch.rs`, `crates/horch-platform/src/channel.rs`, `crates/horch-platform/src/mac/`, `crates/horch-platform/src/linux/`, `crates/horch-platform/src/windows/`, `crates/horch-platform/src/unix/`, `crates/horch-platform/tests/phase1_concerns.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 ("Phase 1 adds the fleetd concerns"), "Architecture and roles" (row Platform layer, table "What each OS provides") and "Migration plan" item 3.

## Goal

`crates/horch-platform` has 5 new concern files for the fleetd concerns of Phase 1, each with its 4 OS files, and the Windows features that tickets 05 to 08 need. Each file is empty except its module doc, so tickets 05 to 08 fill them at the same time.

## Requirements

- **PL-2** (part): the interface gets the Phase 1 concerns: service install and restart, hang detection, sleep and wake, file watching, the fleetd lock, and the transport of the command channel with its peer-user check.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Phase P ticket 01 (`docs/tickets/fleetd-pp/01-platform-crate.md`) made the layout: 1 concern file `src/<concern>.rs`, the OS files `src/mac/<concern>.rs`, `src/linux/<concern>.rs`, `src/windows/<concern>.rs` and the shared `src/unix/<concern>.rs`, and the `mod` lines in `src/mac.rs`, `src/linux.rs`, `src/windows.rs` and `src/unix.rs`. Follow the same layout.
- The 5 new concerns and the tickets that fill them: `lock` (the fleetd lock) and `awake` (sleep and wake: the awake clock) by ticket 05; `service` (service install and restart, and hang detection) by ticket 06; `watch` (file watching) by ticket 07; `channel` (the transport of the command channel, the peer user and the peer process) by ticket 08.
- Only this ticket edits `crates/horch-platform/Cargo.toml` in Phase 1. Add the `windows-sys` features that the Windows modules need: `Win32_Storage_FileSystem` (LockFileEx, ReadDirectoryChangesW), `Win32_System_IO` (overlapped reads), `Win32_System_Pipes` (named pipes, the client process of a pipe), `Win32_System_WindowsProgramming` (QueryUnbiasedInterruptTime). Keep the features of Phase P.

## Steps

1. Write the test first: `crates/horch-platform/tests/phase1_concerns.rs`, test `phase1_concern_modules_exist`, which names `horch_platform::lock`, `awake`, `service`, `watch` and `channel` in `use` lines. It fails to compile before the files exist.
2. Create the 5 concern files and their 20 OS files, each with a module doc that names the concern and the ticket that fills it. Add the `mod` lines to `lib.rs`, `mac.rs`, `linux.rs`, `windows.rs` and `unix.rs`.
3. Add the 4 `windows-sys` features to `crates/horch-platform/Cargo.toml`.
4. Run the Check. Run `cargo check -p horch-platform --target x86_64-pc-windows-msvc` and `--target x86_64-unknown-linux-gnu` only when the target is installed (`rustc --print target-list` does not tell; `rustup` is not on this Mac). CI gives Linux and Windows.

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

The Check passes. The 5 concern files exist with their OS files and hold no code.
