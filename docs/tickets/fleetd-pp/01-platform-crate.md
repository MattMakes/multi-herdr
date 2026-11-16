# 01: Create the crate horch-platform with its layout and the NotSupported value

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-platform && cargo build --workspace --locked`
**Own:** `crates/horch-platform/`, `Cargo.toml`, `Cargo.lock`, `crates/horch-core/Cargo.toml`, `crates/horch/Cargo.toml`, `crates/herdr-install/Cargo.toml`, `crates/horch-marketplace/Cargo.toml`

Spec: `docs/specs/fleetd/design.md`, sections "Architecture and roles" (row Platform layer, paragraph "One platform layer for three OSes") and "Migration plan", item 2 (Phase P).

## Goal

The leaf crate `crates/horch-platform` exists. It has 1 interface file for each concern, 1 module for
each OS, and the explicit value `NotSupported`. Each concern file is empty, so tickets 03 to 09 can
fill them at the same time. The 4 crates that have OS code today depend on it. No behavior changes.

## Requirements

- **PL-2** (part): the platform interface and 1 module for each OS exist in `crates/horch-platform`.
- **PL-6** (part): a gap of an OS is stated as an explicit `NotSupported` value.

## Context

- The design puts the layer in the leaf crate `crates/horch-platform`, with the modules `mac.rs`,
  `linux.rs` and `windows.rs`. It depends on no other crate of the workspace. Each crate calls it.
- 4 crates have OS code today: `horch-core`, `horch`, `herdr-install` and `horch-marketplace`.
  `herdr-install` and `horch-marketplace` do not depend on `horch-core`.
- Today the OS dependencies are target sections in the crate manifests:
  `crates/horch-core/Cargo.toml` (`libc` for unix; `windows-sys` 0.61 with the features
  `Win32_Foundation`, `Win32_Security`, `Win32_System_Diagnostics_ToolHelp`,
  `Win32_System_JobObjects`, `Win32_System_Threading` for windows), `crates/horch/Cargo.toml`
  (`libc` and `tar` for unix, `zip` for windows), `crates/herdr-install/Cargo.toml` (`winreg` 0.52
  for windows; `zip` for all).
- The workspace lint `clippy::undocumented_unsafe_blocks` is on. Each `unsafe` block has a
  `// SAFETY:` note.
- This Mac has only the Rust target `aarch64-apple-darwin` (Homebrew Rust, no rustup). It compiles
  only the macOS module. CI on the draft pull request of `fleet/fleetd-build` compiles Linux and
  Windows at each push.

## Steps

1. Write the tests first, in `crates/horch-platform/tests/not_supported.rs`:
   - `not_supported_names_its_concern_and_os`: `NotSupported { concern: "x", os: "windows" }`
     displays a text that names the concern and the OS. It fails before the crate exists.
   - `not_supported_converts_to_io_unsupported`: `std::io::Error::from(NotSupported { .. })` has
     the kind `std::io::ErrorKind::Unsupported`.
2. Create the layout. Use exactly these files:
   - `crates/horch-platform/Cargo.toml`: `[lints] workspace = true`, version, edition and
     `rust-version` from the workspace. Move no dependency yet. Add the OS dependencies that tickets
     03 to 09 need, with the same versions and features as today: `libc = "0.2"` for
     `cfg(unix)`; `windows-sys = "0.61"` with the 5 features above for `cfg(windows)`;
     `tar = { version = "0.4", default-features = false }` for `cfg(unix)`;
     `zip = { version = "2", default-features = false, features = ["deflate"] }`;
     `winreg = "0.52"` for `cfg(windows)`. Dev-dependency `tempfile = "3"`.
   - `crates/horch-platform/src/lib.rs`: the crate doc, `NotSupported`, the `mod` line of each
     concern file, and the choice of the OS module. Only `lib.rs` chooses the OS module:
     `#[cfg(target_os = "macos")] mod mac;`, `#[cfg(target_os = "linux")] mod linux;`,
     `#[cfg(windows)] mod windows;`, `#[cfg(any(target_os = "macos", target_os = "linux"))] mod unix;`,
     and 1 private alias `os` for the module of the build target. Any other target gets
     `compile_error!("horch-platform supports macOS, Linux and Windows")`.
   - 9 concern files, each with a module doc that names its concern and the ticket that fills it:
     `src/process.rs` (03), `src/signal.rs` (04), `src/group.rs` (05), `src/fs.rs` (06),
     `src/path.rs` (07), `src/ipc.rs` (08), `src/shell.rs` (08), `src/machine.rs` (09),
     `src/rest.rs` (09).
   - `src/mac.rs`, `src/linux.rs`, `src/windows.rs` and `src/unix.rs`: only the `mod` lines of
     their 9 concern files, `src/mac/<concern>.rs`, `src/linux/<concern>.rs`,
     `src/windows/<concern>.rs` and `src/unix/<concern>.rs`. Create the 36 files with 1 module doc
     line each.
3. `NotSupported` is `pub struct NotSupported { pub concern: &'static str, pub os: &'static str }`,
   with `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Display`, `std::error::Error` and
   `From<NotSupported> for std::io::Error` (kind `Unsupported`). A function of a concern returns
   it where the OS has no equivalent. Core code decides what to do with it.
4. Add `horch-platform = { path = "crates/horch-platform" }` to `[workspace.dependencies]` and the
   crate to `[workspace] members` in the root `Cargo.toml`. Add `horch-platform.workspace = true` to
   `[dependencies]` of `crates/horch-core/Cargo.toml`, `crates/horch/Cargo.toml`,
   `crates/herdr-install/Cargo.toml` and `crates/horch-marketplace/Cargo.toml`. Do not remove a
   target dependency of these 4 crates: ticket 16 removes them after the move.
5. Run `cargo build --workspace --locked` after you update `Cargo.lock` with `cargo build`. Run
   the Check.

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

Rules of this phase:

- The crate depends on no crate of the workspace.
- Write no function of a concern. Tickets 03 to 09 own the concern files.
- `unix.rs` is not a 4th OS module. It holds code that `mac.rs` and `linux.rs` share, and only they
  call it.

## Done when

- The Check passes.
- The tree of `crates/horch-platform/src/` has `lib.rs`, the 9 concern files, the 4 OS files and
  the 36 concern files of the OS folders.
- The 4 crates depend on `horch-platform`, and `cargo build --workspace --locked` passes.
