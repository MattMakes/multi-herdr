# 14: Move the harness, probe and pane shell sites of horch-core behind the platform layer

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** full
**Check:** `scripts/pl6/check.sh`
**Own:** `crates/horch-core/src/harness/claude.rs`, `crates/horch-core/src/harness/claude_plugins.rs`, `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/harness/pi.rs`, `crates/horch-core/src/harness/prime.rs`, `crates/horch-core/src/harness/trust.rs`, `crates/horch-core/src/runtime/machine.rs`, `crates/horch-core/src/workspace/paneshell.rs`, `crates/horch-core/src/project_notes.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 and PL-6, and the Migration plan, item 2 (Phase P).

## Goal

The harness adapters (codex, Prime, pi, plugins, trust), the machine probe, the pane shell and the host line call `horch_platform`, and these 9 files have no OS `cfg` in production code. Each test has the same result as before.

## Requirements

- **PL-2** (part): no OS `cfg` in the production code of these files.
- **PL-6** (part): no change of behavior; `scripts/pl6/check.sh` passes.

## Context

The production sites of today (35), from `grep` at commit `09472b9e`. Line numbers move when
other tickets land, so find each site again by its text:

| File | Production sites (line: text) |
| --- | --- |
| `crates/horch-core/src/harness/claude.rs` | `1116`: `#[cfg(unix)]` |
| `crates/horch-core/src/harness/claude_plugins.rs` | `533`: `#[cfg(unix)]`; `539`: `#[cfg(not(unix))]` |
| `crates/horch-core/src/harness/codex.rs` | `156`: `#[cfg(not(windows))]`; `212`: `#[cfg(not(windows))]`; `223`: `#[cfg(windows)]`; `301`: `#[cfg(not(windows))]`; `324`: `#[cfg(not(windows))]`; `365`: `#[cfg(windows)]`; `395`: `#[cfg_attr(not(windows), allow(dead_code))]`; `722`: `if cfg!(windows) {` |
| `crates/horch-core/src/harness/pi.rs` | `61`: `#[cfg(unix)]`; `67`: `#[cfg(not(unix))]` |
| `crates/horch-core/src/harness/prime.rs` | `72`: `#[cfg(unix)]`; `78`: `#[cfg(not(unix))]`; `349`: `#[cfg(unix)]`; `362`: `#[cfg(not(unix))]`; `451`: `if cfg!(windows) {`; `495`: `#[cfg(unix)]`; `505`: `#[cfg(not(unix))]`; `642`: `#[cfg(unix)]`; `652`: `#[cfg(not(unix))]`; `854`: `#[cfg(unix)]`; `859`: `#[cfg(windows)]` |
| `crates/horch-core/src/harness/trust.rs` | `832`: `#[cfg(unix)]`; `847`: `#[cfg(unix)]` |
| `crates/horch-core/src/runtime/machine.rs` | `146`: `#[cfg(unix)]`; `311`: `#[cfg(any(target_os = "linux", target_os = "macos"))]`; `316`: `#[cfg(not(any(target_os = "linux", target_os = "macos")))]`; `322`: `#[cfg(not(unix))]`; `351`: `#[cfg_attr(not(unix), allow(dead_code))]`; `372`: `#[cfg_attr(not(unix), allow(dead_code))]` |
| `crates/horch-core/src/workspace/paneshell.rs` | `33`: `if cfg!(windows) {` |
| `crates/horch-core/src/project_notes.rs` | `99`: `let tools = if cfg!(target_os = "macos") {`; `101`: `} else if cfg!(target_os = "linux") {` |

The platform items come from ticket 03 (`process`), 04 (`signal::terminate_politely`,
`signal::outlive_hangup`), 06 (`fs`), 07 (`path::symlink`, `path::copy_symlink`,
`path::private_link_home_supported`), 08 (`ipc` for `prime.rs:349` `listening`, `shell` for
`paneshell.rs:33`) and 09 (`machine`, `rest::HOST_TOOLS`, the fact of `prime.rs:451`). Read the
module doc of each concern file in `crates/horch-platform/src/` for the final names.

`harness/claude.rs:1116` is inside the test helper `sandbox_tools_path` (`#[cfg(test)]`). Replace
its `#[cfg(unix)]` call of `make_executable` with `horch_platform::fs::make_executable`, which
does nothing on Windows, as the `cfg` does today. Then the `arch_scan` rule of ticket 16 needs no
item-level test rule. `sandbox_host_problem` in `claude.rs` has no `cfg`; it stays (see
`MAP.md`, Q5).

`codex.rs` keeps both rule layouts and chooses by `path::private_link_home_supported()`. Its
messages stay the same. `prime.rs` keeps `may_stop` and its texts. `runtime/machine.rs` keeps
`probe`, `MachineSnapshot` and the fixture path; on `NotSupported` it gives the unknown values of
today (row S12). The tests of `machine.rs` that call `parse_vm_stat` and `parse_meminfo` call the
`pub` functions of `horch_platform::machine`.

## Steps

1. The tests of this ticket come first and exist already: the `grep` of the rules below, which
   prints 35 lines before the move and must print none after it, and
   `scripts/pl6/check.sh`, which must pass before and after. Run both before you change a file.
   If `check.sh` fails before your change, stop and send BLOCKED with its output.
2. Move each site of the table. Keep the logic of horch in these files, and call the platform for
   each OS action.
3. Run the `grep` of the rules below on your 9 files. It prints nothing.
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
