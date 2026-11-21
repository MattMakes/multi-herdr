# Land the work of task T-243 (CI FIX-14: the test targets compile for Windows) with a Check that works on this host

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --tests --locked --target x86_64-pc-windows-msvc && scripts/dev-sandbox.sh cargo test -p horch-platform --locked --test sandbox && scripts/dev-sandbox.sh cargo test --workspace --locked --no-run && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/common/warm.rs`, `crates/horch-core/tests/godot_wave.rs`, `crates/horch-core/tests/i308_triage_apply.rs`, `crates/horch-core/tests/idea_lead_persona.rs`, `crates/horch-core/tests/intake_roster.rs`, `crates/horch-core/tests/p5_claude_sandbox.rs`, `crates/horch-core/tests/p7_claude_exec.rs`, `crates/horch-core/tests/r_workspace_arrange.rs`, `crates/horch-core/tests/status_parked_disk.rs`, `crates/horch-core/tests/triage_apply_job.rs`, `crates/horch-core/tests/w3_finish_steps.rs`, `crates/horch-core/tests/worktree_create.rs`, `crates/horch-core/tests/worktree_jobs.rs`, `crates/horch-e2e/src/process.rs`, `crates/horch-e2e/tests/fleet18_messaging.rs`, `crates/horch-platform/tests/sandbox.rs`, `crates/horch/src/cmd/messaging.rs`, `crates/horch/src/cmd/recipes.rs`, `crates/horch/src/cmd/taskledger_watch.rs`

## Goal

Task T-243 fixed 35 of 36 Windows compile errors of the test targets (commits b2a124b5 and b1ed6c2d). Its Check used the cargo of Homebrew, which has no std for the Windows target, so it could not pass. The local branch `keep/fleetd-finish-T243` (tip b1ed6c2d) holds the work. This task lands it. The last error (`crates/horch-core/tests/harness_defaults.rs:723`) belongs to task T-241.

## Steps

1. In your worktree: `git merge --ff-only keep/fleetd-finish-T243`. If that fails because the run branch moved: `git reset --hard keep/fleetd-finish-T243`, then `git rebase fleet/fleetd-finish`. Resolve a conflict in favor of both changes. The 2 commits of T-243 must be in `git log`.
2. Read the diff. For each `cfg(unix)` gate: confirm that it gates 1 function or 1 test, not more than the cause needs, and that no test of a Windows rule is gated. For `crates/horch/src/cmd/recipes.rs` and `crates/horch/src/cmd/taskledger_watch.rs` (product code): confirm that the commit message says what Windows does now; if it does not, add a commit that says it.
3. Run the Check 1 time. Use the rustup cargo for the Windows check, as the Check does. Install nothing.
4. Run `horch ledger finish`.

## Out of scope

New fixes. The msvc target is proved by the CI.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`.
3. Never install the build or a tool. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Never run `rm -rf /tmp/horch-e2e-*`.
6. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The 2 commits of T-243 are on the run branch and the Check passes.
