# 15: Move the sites of the horch CLI, herdr-install and horch-marketplace behind the platform layer

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** full
**Check:** `scripts/pl6/check.sh`
**Own:** `crates/horch/src/main.rs`, `crates/horch/src/cmd/compact.rs`, `crates/horch/src/cmd/doctor.rs`, `crates/horch/src/cmd/install.rs`, `crates/horch/src/cmd/restore.rs`, `crates/horch/src/cmd/selfupdate.rs`, `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch/src/cmd/taskledger_finish.rs`, `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch/src/cmd/telemetry/mod.rs`, `crates/herdr-install/src/main.rs`, `crates/herdr-install/src/place.rs`, `crates/herdr-install/src/win.rs`, `crates/horch-marketplace/src/source.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 and PL-6, and the Migration plan, item 2 (Phase P).

## Goal

The production code of the crates `horch`, `herdr-install` and `horch-marketplace` has no OS `cfg`, and calls `horch_platform` for each OS action. Each test has the same result as before.

## Requirements

- **PL-2** (part): no OS `cfg` in the production code of these files.
- **PL-6** (part): no change of behavior; `scripts/pl6/check.sh` passes.

## Context

The production sites of today (37), from `grep` at commit `09472b9e`. Line numbers move when
other tickets land, so find each site again by its text:

| File | Production sites (line: text) |
| --- | --- |
| `crates/horch/src/main.rs` | none |
| `crates/horch/src/cmd/compact.rs` | `649`: `#[cfg(unix)]` |
| `crates/horch/src/cmd/doctor.rs` | none |
| `crates/horch/src/cmd/install.rs` | `18`: `if cfg!(windows) {`; `91`: `if cfg!(windows) {`; `113`: `if cfg!(windows) {`; `155`: `if !cfg!(unix) {`; `286`: `swap_in(&staged, target, cfg!(windows), /from, to/ {` |
| `crates/horch/src/cmd/restore.rs` | `25`: `if cfg!(windows) {` |
| `crates/horch/src/cmd/selfupdate.rs` | `376`: `#[cfg(unix)]`; `381`: `#[cfg(not(unix))]`; `577`: `#[cfg(unix)]`; `585`: `#[cfg(unix)]`; `606`: `#[cfg(windows)]` |
| `crates/horch/src/cmd/spawn.rs` | `421`: `#[cfg(target_os = "macos")]`; `443`: `#[cfg(any(target_os = "linux", target_os = "android"))]`; `451`: `#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "a` |
| `crates/horch/src/cmd/taskledger.rs` | `2518`: `shell: PathBuf::from(if cfg!(unix) { "/bin/sh" } else { "sh" }),` |
| `crates/horch/src/cmd/taskledger_finish.rs` | `168`: `#[cfg(unix)]` |
| `crates/horch/src/cmd/taskledger_watch.rs` | `30`: `#[cfg(unix)]`; `39`: `#[cfg(unix)]` |
| `crates/horch/src/cmd/telemetry/mod.rs` | `46`: `#[cfg(unix)]`; `54`: `#[cfg(unix)]` |
| `crates/herdr-install/src/main.rs` | `307`: `if cfg!(windows) {`; `354`: `let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };` |
| `crates/herdr-install/src/place.rs` | `21`: `if cfg!(windows) {`; `99`: `#[cfg(unix)]`; `107`: `#[cfg(not(unix))]` |
| `crates/herdr-install/src/win.rs` | `9`: `#![cfg_attr(not(windows), allow(dead_code))]`; `84`: `#[cfg(windows)]`; `91`: `#[cfg(not(windows))]`; `101`: `#[cfg(windows)]`; `112`: `#[cfg(windows)]`; `135`: `#[cfg(windows)]`; `156`: `#[cfg(not(windows))]`; `165`: `#[cfg(windows)]`; `183`: `#[cfg(not(windows))]` |
| `crates/horch-marketplace/src/source.rs` | `53`: `#[cfg(unix)]`; `63`: `#[cfg(not(unix))]` |

The platform items come from ticket 03 (`process::parent` for `spawn.rs` `parent_pid`), 04
(`signal` handlers for `taskledger_watch.rs` and `telemetry/mod.rs`), 05 (`group::new_group` for
`taskledger_finish.rs:168`), 06 (`fs` modes), 07 (`path`: home variable, `APPDATA`,
`LOCALAPPDATA`, junctions), 08 (`shell::CHECK_SHELL`) and 09 (`rest`: archive, restart, user PATH,
install facts, exe name). Read the module doc of each concern file in
`crates/horch-platform/src/` for the final names.

`herdr-install/src/win.rs` keeps only what is not OS code; its junction and registry bodies are in
the platform. If the file is then empty of logic, delete it and its `mod` line in
`herdr-install/src/main.rs`. Do not leave a file that only re-exports.

The test sites stay: `crates/horch/src/cmd/doctor.rs:873`, `crates/horch/src/cmd/taskledger_watch.rs:603`, `crates/horch/src/main.rs:9`. The `horch` crate keeps its `cfg(all(test, unix))` test modules.

## Steps

1. The tests of this ticket come first and exist already: the `grep` of the rules below, which
   prints 37 lines before the move and must print none after it, and
   `scripts/pl6/check.sh`, which must pass before and after. Run both before you change a file.
   If `check.sh` fails before your change, stop and send BLOCKED with its output.
2. Move each site of the table. Keep the logic of horch in these files, and call the platform for
   each OS action.
3. Run the `grep` of the rules below on your 15 files. It prints nothing.
4. Run `cargo clippy -p horch -p herdr-install -p horch-marketplace --all-targets -- -D warnings` and the Check.

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

Rules of this phase (move tickets 10 to 15):

- This ticket changes no behavior (PL-6). Each test of the workspace has the same result before
  and after it. `scripts/pl6/check.sh` (ticket 02) proves this against
  `scripts/pl6/baseline-macos.txt`.
- Do not delete, rename or move a test. A test keeps its crate, its file and its name, because the
  compare keys each result by them. You may change the body of a test so that it calls the
  platform function in place of the old private one.
- Each `cfg` of the forms `cfg(target_os)`, `cfg(unix)`, `cfg(windows)`, `cfg(target_family)`,
  their `any`, `all` and `not`, `cfg_attr` with them and `cfg!` of them leaves the production
  code of your files. Test code keeps its `cfg`: files under `tests/`, a `#[cfg(test)]` module and
  an item behind `cfg(all(test, ...))`.
- Call `horch_platform` directly. Do not leave a file that only re-exports, and do not add a
  `pub use` of the platform crate: `arc_25_no_shim_modules` in
  `crates/horch-core/tests/arch_scan.rs` refuses both. A function of core may stay when it keeps
  logic of its own (for example the error text of today).
- Core keeps the error text of today. Where the platform gives `NotSupported`, core maps it to the
  message, the value or the no-op that the old `cfg` branch gave.
- Do not add a function to `crates/horch-platform/`. If a platform function is missing or wrong,
  send QUESTION to the orchestrator with the function that you need.
- Do not edit a `Cargo.toml`. Ticket 01 added `horch-platform` to each crate. Ticket 16 removes
  the target dependencies that the move leaves unused.
- After the move, run `grep -nE 'cfg!?\(.*(target_os|unix|windows|target_family)' <each file of Own>`.
  Each line that it prints is test code, or the ticket is not done.
- Make the code compile for Linux and Windows as far as this Mac can check. When
  `rustup target list --installed` shows the target, also run
  `cargo check --workspace --target x86_64-unknown-linux-gnu` and
  `cargo check --workspace --target x86_64-pc-windows-msvc`. Today this Mac has Homebrew Rust
  with only `aarch64-apple-darwin`. CI on the draft pull request of `fleet/fleetd-build`
  compiles and tests Linux and Windows at each push. After this ticket lands, a CI fault on Linux
  or Windows comes back as a fix task.

## Done when

- The Check passes.
- The files of `Own` have no OS `cfg` in production code.
