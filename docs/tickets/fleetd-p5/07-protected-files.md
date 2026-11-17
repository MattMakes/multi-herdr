# 07: The protected file reader: the owner, mode, link, real path and size checks of SR-5 and SR-6, in 1 place

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test protected_files && cargo test -p horch-platform && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs`, `crates/horch-core/tests/protected_files.rs`, `crates/horch-platform/src/fs.rs`, `crates/horch-platform/src/mac/fs.rs`, `crates/horch-platform/src/unix/fs.rs`, `crates/horch-platform/src/windows/fs.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

fleetd reads each untrusted file through 1 function that refuses a link, a file that is not regular, a file of another owner, a mode wider than the rule, a real path outside its root, and a file over its size limit, and gives the reason.

## Requirements

- SR-5 (the owner and the mode of a request folder and a request file)
- SR-6 (the checks of a file that a worker writes)
- FM-23

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- Today `crates/horch-platform/src/fs.rs` has `owner_and_mode`, `mode_of` and `current_uid`, but no open that refuses a link. Add `open_no_follow(path) -> io::Result<File>` (`O_NOFOLLOW` on macOS and Linux; `FILE_FLAG_OPEN_REPARSE_POINT` and a check of the reparse tag on Windows).
- The checks of SR-6 and of Protocol rule 7, in this order: the file is regular (`symlink_metadata`); it opens without following a link; the metadata of the open handle equals the metadata of the path (no swap between the 2 calls); its real path is inside the root that the caller gives; its owner is the user that owns the ledger; its mode has no bit for group or others (a folder: 0700, a file: 0600); its size is under the limit of its type. The caller gives the root, the owner and the limit.
- On Windows the owner check reads the owner SID of the file. A fact that the platform cannot give is `NotSupported`, and a guard whose fact is unknown does not hold: the file is refused with the reason "owner unknown".
- The function returns the bytes, or `Rejected { reason }`. The caller records a rejected path in `ingested` (from ticket 02 of fleetd-p4), so the path is not read again.
- Ticket 13 of fleetd-p4 has its own SR-6 reader in `crates/horch-core/src/fleetd/task_execution/adapters/result_reader.rs`. Ticket 29 moves it to this function. Do not change it here.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/protected_files.rs`: `symlink_is_rejected`, `fifo_is_rejected`, `file_over_its_limit_is_rejected_without_reading_it_all`, `file_with_mode_0644_is_rejected`, `folder_with_mode_0755_is_rejected`, `real_path_outside_the_root_is_rejected` (a link in a parent folder), `file_of_another_owner_is_rejected` (the owner comes through the test seam of the caller, because a test cannot make a file of another user), `valid_file_is_read`. Each rejected case asserts its reason text.
2. Add `open_no_follow` to the platform layer, with the code of each OS.
3. Write `protected_files.rs` with 1 public function and its `Rejected` reasons.

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
- The 8 tests pass.
- No other file of horch-core opens an untrusted file without this function (ticket 42 adds the rule).
