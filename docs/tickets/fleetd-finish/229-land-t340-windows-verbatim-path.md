# Land the work of task T-340 (CI4 FIX-36: a Windows verbatim path never reaches a child) with a Check that works on this host

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --all-targets --locked --target x86_64-pc-windows-msvc && scripts/dev-sandbox.sh cargo test -p horch-platform --locked --test path --test start && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test bisect --test push_dispatch && cargo fmt --all --check`
**Own:** `crates/horch-platform/src/path.rs`, `crates/horch-platform/src/start.rs`, `crates/horch-platform/tests/path.rs`, `crates/horch-core/tests/bisect.rs`

## Goal

Task T-340 did the work: commit a3c6bf5e on the local branch `keep/fleetd-finish-T340` (`horch_platform::path::plain_of_verbatim` and `for_child`; `start::Command` `arg`, `args` and `current_dir` use `for_child`; `bisect.rs:96` and `launch.rs:587` use it). Its Check could not pass on this Mac (the Windows check of `horch-core` needs the Windows SDK). Land it.

## Steps

1. In your worktree: `git merge --ff-only keep/fleetd-finish-T340` (if the run branch moved, `git rebase fleet/fleetd-finish` after the merge; resolve a conflict in favor of both changes).
2. Read the commit and its tests. Check 3 cases in `crates/horch-platform/tests/path.rs` and add a test for each that is missing: `\\?\C:\dir` gives `C:\dir`; `\\?\UNC\server\share` gives `\\server\share`; a path that is not valid without the prefix (longer than the legacy limit, a reserved name such as `CON`, a trailing dot or space) keeps the prefix.
3. Run the Check. Run `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan`: 2 tests are red on the base (`fitness_no_context_touches_foreign_tables`, owner T-327; `fitness_workspace_production_code_has_no_os_cfg_outside_horch_platform` at `conformance_runner.rs:769` and `:776`, owner T-347); this commit must add no new line to them.
4. Run `horch ledger finish`. After the comment sweep, restore each comment that says why a path keeps its prefix.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build or a tool. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never read, set or pass `ANTHROPIC_API_KEY`.
4. No flaky test. Do not weaken an assert. No `#[ignore]`.
5. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet.
6. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and the commits of `keep/fleetd-finish-T340` are on the run branch.
