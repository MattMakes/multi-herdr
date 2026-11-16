# 07: Platform concern: paths, home folders, symlinks and links

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-platform --test path && cargo clippy -p horch-platform --all-targets -- -D warnings`
**Own:** `crates/horch-platform/src/path.rs`, `crates/horch-platform/src/mac/path.rs`, `crates/horch-platform/src/linux/path.rs`, `crates/horch-platform/src/windows/path.rs`, `crates/horch-platform/src/unix/path.rs`, `crates/horch-platform/tests/path.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 (the concerns "paths and home directories" and
"symlinks and links"), and `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6,
rows "Paths and home dirs" and "Symlinks".

## Goal

`horch_platform::path` gives the name of the home variable, the home folder of the passwd
database, the fixed paths of today (Claude managed settings, the app bundles of Blender and
Godot, the herdr config folder variable), symlinks, the copy of a symlink, Windows junctions and
reparse points, and the fact whether a private `CODEX_HOME` of symlinks is possible. The behavior
is the same as today on each OS.

## Requirements

- **PL-2** (part): the concerns "paths and home directories" and "symlinks and links" are in the
  platform interface.
- **PL-6** (part): the gaps of today are explicit `NotSupported` values.

## Context

Today's code that this ticket copies (do not edit these files):

| Today | What it does | Platform item (proposed name) |
| --- | --- | --- |
| `crates/horch-core/src/runtime/paths.rs:148` `home_dir`, `crates/herdr-install/src/main.rs:354` `home_dir` | the variable `HOME`, or `USERPROFILE` on Windows | `path::HOME_VAR: &str` (a constant; the caller reads the variable, because `arc_05_no_ambient_env_in_core` in `crates/horch-core/tests/arch_scan.rs` keeps env reads in `runtime/`) |
| `crates/horch-core/src/taskledger/store.rs:84`, `:112` `passwd_home` | `getpwuid_r` home; `None` on Windows, so no ledger counts as shared (`store.rs:112`) | `path::passwd_home()`; Windows gives `NotSupported` |
| `crates/horch-core/src/runtime/paths.rs:50`, `:52` `CLAUDE_MANAGED_SETTINGS` | 3 fixed paths | `path::CLAUDE_MANAGED_SETTINGS: &str` |
| `crates/horch-core/src/runtime/bins.rs:215` `BLENDER_APP`, `:223` `GODOT_APP`, `:231` `GODOT_MONO_APP` | app bundle paths, macOS only | `path::BLENDER_APP`, `GODOT_APP`, `GODOT_MONO_APP: Option<&str>` (`None` off macOS, as today) |
| `crates/horch/src/cmd/restore.rs:25` | `%APPDATA%\herdr\config.toml` on Windows | `path::CONFIG_BASE_VAR: Option<&str>` (`Some("APPDATA")` on Windows, `None` elsewhere); read the function around `restore.rs:25` and keep its order of lookups |
| `crates/horch/src/cmd/install.rs:18` `default_dir` | `%LOCALAPPDATA%\Programs\horch` on Windows | a fact of the same form; keep the order of today |
| `crates/horch-core/src/harness/prime.rs:854`, `:859` `link` | symlink; on Windows `symlink_dir` or `symlink_file` | `path::symlink(target, at)` |
| `crates/horch-core/src/harness/claude_plugins.rs:533`, `:539` `copy_symlink` | copy a symlink; refused on Windows | `path::copy_symlink(from, to)`; Windows gives `NotSupported`, and the caller keeps its message |
| `crates/horch-core/src/harness/codex.rs:156` `clear_skills_link`, `:212` and `:223` `attach_skills`, `:301` and `:365` `install_rules`, `:324` `build_private_home`, `:395` `ensure_rules`, `:722` `ensure_skills_supported` | a private `CODEX_HOME` of symlinks; on Windows no private home and the shared rules file | `path::private_link_home_supported() -> bool` (false on Windows, with row "Symlinks"), plus `path::symlink` |
| `crates/herdr-install/src/win.rs:84` and `:91` `is_reparse_point`, `:101` and `:156` `set_junction`, `:112` `clear_link`, `:135` `mklink_junction` | Windows junctions | `path::is_reparse_point`, `path::set_junction`, `path::clear_link`; macOS and Linux give `NotSupported` |

`codex.rs` keeps the choice between the 2 rule layouts. It asks the platform the fact
`private_link_home_supported`, not the OS name.

## Steps

1. Write the tests first, in `crates/horch-platform/tests/path.rs`. Each fails before the items
   exist:
   - `home_var_is_home_off_windows` (`#[cfg(unix)]`) and `home_var_is_userprofile_on_windows`
     (`#[cfg(windows)]`, run by CI).
   - `passwd_home_is_absolute` (`#[cfg(unix)]`) and `passwd_home_is_not_supported_on_windows`
     (`#[cfg(windows)]`).
   - `symlink_points_at_its_target` and `copy_symlink_copies_the_target_text` (`#[cfg(unix)]`).
   - `copy_symlink_is_not_supported_on_windows` (`#[cfg(windows)]`).
   - `private_link_home_is_supported_off_windows` (`#[cfg(unix)]`).
   - `junctions_are_not_supported_off_windows` (`#[cfg(unix)]`) and
     `set_junction_then_clear_link` (`#[cfg(windows)]`).
   - `fixed_paths_match_today`: each constant has the value of today on the host OS.
2. Write `src/path.rs` and the 3 OS modules. Shared unix code goes in `src/unix/path.rs`. The
   constants are `pub const` items of each OS module that `src/path.rs` exports by name.
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
- Each row of the table has its platform item. Each gap is `NotSupported` or is stated in the doc
  of its item with the row of section 6.
