# 16: The arch_scan rule of PL-2, and the removal of the unused OS dependencies

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-core --test arch_scan && scripts/pl6/check.sh`
**Own:** `crates/horch-core/tests/arch_scan.rs`, `crates/horch-core/Cargo.toml`, `crates/horch/Cargo.toml`, `crates/herdr-install/Cargo.toml`, `crates/horch-marketplace/Cargo.toml`, `Cargo.lock`

Spec: `docs/specs/fleetd/design.md`, PL-2: "From Phase P, an `arch_scan` rule finds no OS `cfg` in
production code outside the platform layer. The rule names each site that cannot call the layer
(a build script) by path, with its reason."

## Goal

1 test of `crates/horch-core/tests/arch_scan.rs` fails when production code of the workspace
outside `crates/horch-platform` has an OS `cfg`. It names the build script site by path with its
reason. The crates that no longer use an OS dependency do not declare it.

## Requirements

- **PL-2**: the `arch_scan` rule.

## Context

- `crates/horch-core/tests/arch_scan.rs` has the helpers `crate_src`, `rust_files`, `code_lines`
  and `relative`. `code_lines` skips comment lines, a file named `tests.rs`, and the rest of a
  file after `#[cfg(test)]` when the next line starts with `mod `. It does not see
  `#[cfg(unix)]` then `#[cfg(test)]` then `mod tests` (`crates/horch/src/cmd/taskledger_watch.rs:603`,
  `crates/horch-core/src/messaging/channel.rs:750`), nor `#[cfg(all(test, unix))] mod tests`
  (`crates/horch/src/cmd/doctor.rs:873`, `crates/horch-core/src/harness/pi.rs:209`,
  `crates/horch/src/main.rs:9`, `crates/horch-core/src/lib.rs:70`). Other rules use `code_lines`,
  so do not change its result for them: add the new skips in a new helper.
- The OS forms to find: `cfg(...)`, `cfg_attr(...)` and `cfg!(...)` whose predicate names
  `target_os`, `unix`, `windows` or `target_family`, also inside `any`, `all` and `not`, and an
  attribute over more than 1 line (`#[cfg(not(any(` at `crates/horch-core/src/procid.rs:450`
  today).
- The scope: each crate of the workspace members in the root `Cargo.toml`, its `src/` and its
  `build.rs`. Not in scope: `crates/horch-platform/` (the layer), `crates/horch-e2e/` (the fake
  harness and the end-to-end tests: test code), each `tests/` folder, each `tests.rs`, each
  `#[cfg(test)]` module and each item behind a `cfg` that names `test`.
- The 1 exempt site: `crates/horch-core/build.rs` (lines 112 and 117 today). Its reason: a build
  script runs on the build host before the crate compiles, so it cannot call the platform layer
  of the target. The rule lists it by path with this reason, and fails when the file has no OS
  `cfg` any more, so the list stays true.
- Today the OS dependencies are target sections: `libc` and `windows-sys` in
  `crates/horch-core/Cargo.toml`; `libc`, `tar` and `zip` in `crates/horch/Cargo.toml`; `winreg`
  in `crates/herdr-install/Cargo.toml`. After tickets 10 to 15, production code of these crates
  may use none of them. A test that still uses one keeps it as a dev-dependency of the same
  target (`[target.'cfg(unix)'.dev-dependencies]`).

## Steps

1. Write the tests first, in `crates/horch-core/tests/arch_scan.rs`:
   - `pl2_scan_finds_each_os_cfg_form_and_skips_test_code`: fixed texts with each form above, a
     multi-line attribute, a `#[cfg(unix)] #[cfg(test)] mod tests`, a
     `#[cfg(all(test, unix))] mod tests` and a comment. It fails before the new helper exists.
   - `fitness_workspace_production_code_has_no_os_cfg_outside_horch_platform`: scan the scope
     above. It prints each site as `<crate>/<path>:<line>: <text>` and fails when 1 exists. It
     passes only after tickets 10 to 15 land. To see it fail for the correct reason, make a
     scratch worktree (`horch ledger worktree scratch pl2-base`), run
     `git checkout --detach <hash of scripts/pl6/BASE>` there, copy your `arch_scan.rs` into it,
     and run `cargo test -p horch-core --test arch_scan` there. It names about 205 sites (the
     count of `docs/tickets/fleetd-pp/MAP.md`) and not the exempt build script.
   - `pl2_exempt_build_script_still_needs_its_exemption`: the exempt file still has an OS `cfg`.
2. Write the helper and the rule.
3. Remove each target dependency that no crate code uses any more (step "Context"). Run
   `cargo build --workspace --locked` after `cargo build` updates `Cargo.lock`.
4. Run the Check.

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

- This ticket changes no behavior (PL-6). `scripts/pl6/check.sh` passes.
- Do not delete, rename or move an existing test of `arch_scan.rs`.
- The rule has no list of allowed sites other than the 1 build script with its reason. If a site
  is left in a file of tickets 10 to 15, send QUESTION to the orchestrator: a fix task for that
  ticket moves it. Do not move it here.

## Done when

- The Check passes, and the new rule is part of it.
- The new rule fails on the tree of `scripts/pl6/BASE` and names each site.
- No crate declares an OS dependency that its code does not use.
