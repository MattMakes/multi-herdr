# 06: Platform concern: file locks, file permissions, file owner and file identity

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-platform --test fs && cargo clippy -p horch-platform --all-targets -- -D warnings`
**Own:** `crates/horch-platform/src/fs.rs`, `crates/horch-platform/src/mac/fs.rs`, `crates/horch-platform/src/linux/fs.rs`, `crates/horch-platform/src/windows/fs.rs`, `crates/horch-platform/src/unix/fs.rs`, `crates/horch-platform/tests/fs.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 (the concerns "file locks", "file permissions" and, from
"the small rest", "file identity"), and `docs/specs/fleetd/preservation-06-wiring-and-platform.md`,
section 6, rows "File locks", "File permissions" and "Other" (`telemetry/cursor.rs:84`).

## Goal

`horch_platform::fs` sets and reads file modes, the owner of a file and the current user, makes a
file executable, creates private files and folders, syncs a folder, opens a regular file with no
block on a FIFO, renames with no replace, holds the lock-breaker guard, and gives the identity of
a file. The behavior is the same as today on each OS. These functions guard private files and the
ledger locks, so this ticket has the hardening persona and a full review.

## Requirements

- **PL-2** (part): the concerns "file locks", "file permissions" and "file identity" are in the
  platform interface.
- **PL-6** (part): the gaps of today are explicit `NotSupported` values.

## Context

Today's code that this ticket copies (do not edit these files):

| Today | What it does | Platform function (proposed name) |
| --- | --- | --- |
| `crates/horch-core/src/fsx.rs:100` `set_mode` (`:101`, `:109`) | `chmod`; ignored on Windows | `fs::set_mode(path, mode)`; Windows gives `NotSupported` |
| `fsx.rs:118` `create_new_file` (`:121`, `:126`) | `create_new` with a mode | `fs::create_mode(&mut OpenOptions, mode)`; Windows gives `NotSupported` |
| `fsx.rs:133` `sync_dir` (`:134`, `:139`) | `fsync` of a folder; a no-op on Windows | `fs::sync_dir(dir)`; Windows gives `NotSupported` |
| `fsx.rs:183` `open_regular` (`:186`) | open with `O_NONBLOCK` so a FIFO does not block | `fs::open_no_block(&mut OpenOptions)`; Windows gives `NotSupported` |
| `fsx.rs:611` `rename_noreplace` (`:612`, `:618` `renamex_np(RENAME_EXCL)`, `:621` `renameat2(RENAME_NOREPLACE)`) | atomic rename that fails when the target exists | `fs::rename_noreplace(from, to)`; read the code after `:621` for the fallback of today and keep it |
| `fsx.rs:683` `BreakerGuard` (`:692` `take` with `flock`, `:713` Windows holds nothing, `:725` `Drop`) | the guard of the lock breaker | `fs::BreakerGuard`; Windows: the guard holds nothing, stated with row "File locks" |
| `crates/horch-core/src/messaging/private.rs:76` `facts`, `:85` `me`, `:94`, `:108`, `:118`, `:129`, `:141`, `:162` | mode and owner checks, private folders mode 0700 and files mode 0600; Windows skips each check | `fs::owner_and_mode(&Metadata)`, `fs::current_uid()`, `fs::create_dir_mode(&mut DirBuilder, mode)`; Windows gives `NotSupported` |
| `crates/horch-core/src/harness/prime.rs:72`, `:78` `real_uid` | `getuid`; a fixed value on Windows | `fs::current_uid()` |
| `crates/horch-core/src/runtime/process.rs:105`, `:113` `is_executable`; `:121`, `:128` `make_executable` | the exec bit; on Windows a file that exists | `fs::is_executable(&Metadata)` and `fs::make_executable(path)`; Windows: "exists" is the rule, stated with row "File permissions" |
| `crates/horch-core/src/skills/stable.rs:81`, `:94` `verify_executable` | read the exec bit | `fs::mode_of(&Metadata)`; Windows gives `NotSupported` |
| `crates/horch-core/src/skills/materialize.rs:157`, `crates/horch-marketplace/src/source.rs:53`, `:63` `set_executable`, `crates/herdr-install/src/place.rs:99`, `:107` | set mode 0755 | `fs::set_mode` |
| `crates/horch-core/build.rs:112`, `:117` | set mode 0755 in the build script | none: a build script cannot call the layer; ticket 16 names it in the `arch_scan` rule |
| `crates/horch-core/src/compaction/jobfile.rs:231`, `crates/horch-core/src/harness/update.rs:425`, `crates/horch-core/src/telemetry/store.rs:163` `private_options`, `crates/horch/src/cmd/compact.rs:649` `open_log` | create with mode 0600 | `fs::create_mode` |
| `crates/horch-core/src/harness/trust.rs:832`, `:847` | copy the mode of a store, then set it again after the umask | `fs::mode_of` and `fs::set_mode` |
| `crates/horch-core/src/harness/pi.rs:61`, `:67` `private_mode` | read whether a file is private | `fs::mode_of` |
| `crates/horch-core/src/telemetry/cursor.rs:84`, `:89`, `:94` `file_identity` | `(dev, ino)` on Unix; on Windows the creation time stands in | `fs::file_identity(&Metadata)` |

Each caller is in a move ticket (10 to 15). This ticket only gives the functions.

## Steps

1. Write the tests first, in `crates/horch-platform/tests/fs.rs`. Each fails before the functions
   exist:
   - `create_mode_gives_0600` and `create_dir_mode_gives_0700` (`#[cfg(unix)]`), also with a
     umask of 0o022.
   - `owner_and_mode_name_the_current_user` (`#[cfg(unix)]`).
   - `rename_noreplace_refuses_an_existing_target` and `rename_noreplace_moves_to_a_free_name`.
   - `breaker_guard_is_exclusive` (`#[cfg(unix)]`): a second `take` in the same process returns
     `None` while the first lives.
   - `open_no_block_does_not_block_on_a_fifo` (`#[cfg(unix)]`).
   - `file_identity_differs_for_2_files_and_is_stable_for_1`.
   - `is_executable_follows_the_exec_bit` (`#[cfg(unix)]`).
   - `modes_and_owner_are_not_supported_on_windows` (`#[cfg(windows)]`, run by CI): `set_mode`,
     `create_mode`, `owner_and_mode`, `current_uid`, `sync_dir`, `open_no_block` give
     `NotSupported`.
2. Write `src/fs.rs` and the 3 OS modules. Shared unix code goes in `src/unix/fs.rs`.
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
- Each row of the table has its platform function. Each Windows gap is `NotSupported` or is
  stated in the doc of its function with the row of section 6.
