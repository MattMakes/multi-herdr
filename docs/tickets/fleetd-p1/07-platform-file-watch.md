# 07: Platform: file watching as a speed-up

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-platform`
**Own:** `crates/horch-platform/src/watch.rs`, `crates/horch-platform/src/mac/watch.rs`, `crates/horch-platform/src/linux/watch.rs`, `crates/horch-platform/src/windows/watch.rs`, `crates/horch-platform/src/unix/watch.rs`, `crates/horch-platform/tests/watch.rs`

Spec: `docs/specs/fleetd/design.md`, table "What each OS provides" (File watching: "Events are only a speed-up on every OS"), PL-2, PL-3, RR-4, FM-7; "Supervision, liveness and reconciliation" (A pass has a work budget: "File watching puts a changed outbox at the front of the order").

## Goal

The platform tells fleetd which watched folders changed since the last read, with no wait. A missed or dropped event costs nothing, because the pass scans each folder in turn anyway.

## Requirements

- **PL-2** (part): the concern "file watching".
- **PL-3** (part): a watch that the OS cannot give returns `NotSupported`, and core scans without it.
- **RR-4** (part): events are only a hint. This ticket gives the hint. Ticket 28 proves that a pass with no events still completes each task.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Proposed names: `watch::Watcher::new() -> io::Result<Watcher>`, `Watcher::add(&mut self, dir: &Path) -> io::Result<()>`, `Watcher::remove(&mut self, dir)`, `Watcher::changed(&mut self) -> Vec<PathBuf>` (the watched folders with a change since the last call; never blocks; may give a folder that did not change; may miss one).
- OS facts: macOS kqueue with `EVFILT_VNODE` on an open folder (FSEvents needs a run loop; kqueue does not); Linux `inotify` (`IN_CREATE | IN_MODIFY | IN_MOVED_TO | IN_CLOSE_WRITE`) with a non-blocking fd; Windows `ReadDirectoryChangesW` with overlapped I/O and a poll with timeout 0. `libc` has the kqueue and inotify calls. Add no new crate.
- A watch of many folders can hit an OS limit (open files for kqueue, `max_user_watches` for inotify). Then `add` returns the error, and the caller goes on without a watch for that folder.

## Steps

1. Write the tests first, in `crates/horch-platform/tests/watch.rs`. Each fails before the code exists:
   - `a_new_file_in_a_watched_folder_is_reported` (poll `changed` for at most 2 s).
   - `an_unwatched_folder_is_not_reported`.
   - `changed_never_blocks` (a call with no change returns in under 50 ms).
   - `a_removed_folder_is_not_an_error`.
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

The Check passes on macOS. `changed` never blocks.
