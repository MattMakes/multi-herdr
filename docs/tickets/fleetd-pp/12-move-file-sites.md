# 12: Move the file, path and home sites of horch-core behind the platform layer

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** full
**Check:** `scripts/pl6/check.sh`
**Own:** `crates/horch-core/src/fsx.rs`, `crates/horch-core/src/messaging/private.rs`, `crates/horch-core/src/compaction/jobfile.rs`, `crates/horch-core/src/skills/materialize.rs`, `crates/horch-core/src/skills/stable.rs`, `crates/horch-core/src/telemetry/cursor.rs`, `crates/horch-core/src/telemetry/store.rs`, `crates/horch-core/src/taskledger/store.rs`, `crates/horch-core/src/runtime/paths.rs`, `crates/horch-core/src/runtime/bins.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 and PL-6, and the Migration plan, item 2 (Phase P).

## Goal

The file mode, owner, lock, file identity, home and fixed path code of these 10 files calls `horch_platform`, and the files have no OS `cfg` in production code. Each test has the same result as before.

## Requirements

- **PL-2** (part): no OS `cfg` in the production code of these files.
- **PL-6** (part): no change of behavior; `scripts/pl6/check.sh` passes.

## Context

The production sites of today (42), from `grep` at commit `09472b9e`. Line numbers move when
other tickets land, so find each site again by its text:

| File | Production sites (line: text) |
| --- | --- |
| `crates/horch-core/src/fsx.rs` | `101`: `#[cfg(unix)]`; `109`: `#[cfg(not(unix))]`; `121`: `#[cfg(unix)]`; `126`: `#[cfg(not(unix))]`; `134`: `#[cfg(unix)]`; `139`: `#[cfg(not(unix))]`; `186`: `#[cfg(unix)]`; `324`: `#[cfg(unix)]`; `331`: `#[cfg(windows)]`; `337`: `#[cfg(not(any(unix, windows)))]`; `346`: `#[cfg(unix)]`; `358`: `#[cfg(not(unix))]`; `612`: `#[cfg(any(target_os = "macos", target_os = "linux"))]`; `618`: `#[cfg(target_os = "macos")]`; `621`: `#[cfg(target_os = "linux")]`; `692`: `#[cfg(unix)]`; `713`: `#[cfg(not(unix))]`; `725`: `#[cfg(unix)]` |
| `crates/horch-core/src/messaging/private.rs` | `76`: `#[cfg(unix)]`; `85`: `#[cfg(unix)]`; `94`: `#[cfg(unix)]`; `108`: `#[cfg(not(unix))]`; `118`: `#[cfg(unix)]`; `129`: `#[cfg(not(unix))]`; `141`: `#[cfg(unix)]`; `162`: `#[cfg(unix)]` |
| `crates/horch-core/src/compaction/jobfile.rs` | `231`: `#[cfg(unix)]` |
| `crates/horch-core/src/skills/materialize.rs` | `157`: `#[cfg(unix)]` |
| `crates/horch-core/src/skills/stable.rs` | `81`: `#[cfg(unix)]`; `94`: `#[cfg(not(unix))]` |
| `crates/horch-core/src/telemetry/cursor.rs` | `84`: `#[cfg(unix)]`; `89`: `#[cfg(windows)]`; `94`: `#[cfg(not(any(unix, windows)))]` |
| `crates/horch-core/src/telemetry/store.rs` | `163`: `#[cfg(unix)]` |
| `crates/horch-core/src/taskledger/store.rs` | `84`: `#[cfg(unix)]`; `112`: `#[cfg(not(unix))]` |
| `crates/horch-core/src/runtime/paths.rs` | `50`: `pub(crate) const CLAUDE_MANAGED_SETTINGS: &str = if cfg!(target_os = "`; `52`: `} else if cfg!(windows) {`; `148`: `let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };` |
| `crates/horch-core/src/runtime/bins.rs` | `215`: `pub const BLENDER_APP: Option<&str> = if cfg!(target_os = "macos") {`; `223`: `pub const GODOT_APP: Option<&str> = if cfg!(target_os = "macos") {`; `231`: `pub const GODOT_MONO_APP: Option<&str> = if cfg!(target_os = "macos") ` |

The platform functions come from ticket 03 (`process::alive` for `fsx::pid_alive`), 06 (`fs`),
07 (`path`) and 09 (`machine::host_name` for `fsx.rs:345`). Read the module doc of each concern file
in `crates/horch-platform/src/` for the final names.

`messaging/private.rs` keeps its checks and its messages. On Windows it skips each check as today
(row "File permissions": no private-file or foreign-owner protection). `taskledger/store.rs` keeps
`guard`; `passwd_home` becomes a call of `path::passwd_home` with `None` on `NotSupported`.
`runtime/paths.rs` keeps the env read through `EnvSource` and takes only the variable name from
`path::HOME_VAR` (`arc_05_no_ambient_env_in_core` in `crates/horch-core/tests/arch_scan.rs`).

## Steps

1. The tests of this ticket come first and exist already: the `grep` of the rules below, which
   prints 42 lines before the move and must print none after it, and
   `scripts/pl6/check.sh`, which must pass before and after. Run both before you change a file.
   If `check.sh` fails before your change, stop and send BLOCKED with its output.
2. Move each site of the table. Keep the logic of horch in these files, and call the platform for
   each OS action.
3. Run the `grep` of the rules below on your 10 files. It prints nothing.
4. Run `cargo clippy -p horch-core --all-targets -- -D warnings` and the Check.

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
