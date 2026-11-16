# 11: Move the herdr call runner of horch-core behind the platform layer

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** full
**Check:** `scripts/pl6/check.sh`
**Own:** `crates/horch-core/src/workspace/herdr.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 and PL-6, and the Migration plan, item 2 (Phase P).

## Goal

The herdr call runner in `workspace/herdr.rs` (a new process group or a Windows job object, the wait with a deadline, the kill of the whole group) calls `horch_platform::group`, and the file has no OS `cfg` in production code. Each test has the same result as before.

## Requirements

- **PL-2** (part): no OS `cfg` in the production code of these files.
- **PL-6** (part): no change of behavior; `scripts/pl6/check.sh` passes.

## Context

The production sites of today (16), from `grep` at commit `09472b9e`. Line numbers move when
other tickets land, so find each site again by its text:

| File | Production sites (line: text) |
| --- | --- |
| `crates/horch-core/src/workspace/herdr.rs` | `676`: `#[cfg(windows)]`; `679`: `#[cfg(windows)]`; `721`: `#[cfg(windows)]`; `748`: `#[cfg(windows)]`; `796`: `#[cfg(unix)]`; `799`: `#[cfg(not(unix))]`; `834`: `#[cfg(unix)]`; `851`: `#[cfg(target_os = "linux")]`; `853`: `#[cfg(not(target_os = "linux"))]`; `857`: `#[cfg(not(unix))]`; `876`: `#[cfg(unix)]`; `887`: `#[cfg(windows)]`; `893`: `#[cfg(windows)]`; `907`: `#[cfg(windows)]`; `936`: `#[cfg(windows)]`; `953`: `#[cfg(windows)]` |

The platform functions come from ticket 05 (`group`: `new_group`, `Contained`, `kill_child_group`,
`exited_without_reap`). Read the module doc of `crates/horch-platform/src/group.rs` for the final
names.

`output_within`, `reap_within`, `join_readers` and the deadline logic stay in `herdr.rs`. The
Windows `Job` type and `resume_main_thread` go away from `herdr.rs`: the platform owns them. A
herdr call that hangs must still be killed with all its children at the deadline on each OS.

## Steps

1. The tests of this ticket come first and exist already: the `grep` of the rules below, which
   prints 16 lines before the move and must print none after it, and
   `scripts/pl6/check.sh`, which must pass before and after. Run both before you change a file.
   If `check.sh` fails before your change, stop and send BLOCKED with its output.
2. Move each site of the table. Keep the logic of horch in these files, and call the platform for
   each OS action.
3. Run the `grep` of the rules below on your 1 file. It prints nothing.
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
