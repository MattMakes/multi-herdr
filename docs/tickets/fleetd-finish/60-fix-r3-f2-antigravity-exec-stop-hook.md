# 60: Fix R3-F2: seed the Antigravity Stop hook on exec-mode push launches (FM-6)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib harness::antigravity && scripts/dev-sandbox.sh cargo test -p horch-core --test harness_defaults && scripts/dev-sandbox.sh cargo test -p horch-core --test p7_antigravity_exec && cargo fmt --all --check`
**Own:** `crates/horch-core/src/harness/antigravity.rs`, `crates/horch-core/src/harness/launch.rs`, `crates/horch-core/tests/p7_antigravity_exec.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p4.md` (finding R3-F2) and `docs/specs/fleetd/design.md` (rule FM-6 and section "How each adapter applies a profile", rows `mode: exec` and `result_gate`).

## Read first: what task T-16 already landed on `fleet/fleetd-finish`

Task T-16 (`16-d1-antigravity-stop-hook.md`, commits `b1382d1a`, `2f21c38b`, and `df3f9299` on `fleet/fleetd-finish`) implemented `seed_stop_hook(worktree: &Path, horch_exe: &Path)` in `crates/horch-core/src/harness/antigravity.rs:106-137` and wired it into the interactive push launch `Antigravity::command` at `crates/horch-core/src/harness/antigravity.rs:205-207`, with unit test `the_stop_hook_maps_exit_2_to_continue` (`crates/horch-core/src/harness/antigravity.rs:569`) and adapter tests `antigravity_push_launch_has_a_stop_hook_that_runs_result_check`, `antigravity_push_launch_keeps_a_tracked_hooks_file`, `antigravity_push_launch_writes_through_no_symlink`, and `claim_attempt_has_no_stop_hook` (`crates/horch-core/tests/harness_defaults.rs:873-944`).

However, R3-F2 also cites the exec-mode launch path (`crates/horch-core/src/harness/antigravity.rs:148` at review time, now line `247`, `pub fn exec_command(launch: &ExecLaunch<'_>) -> Command`, called from `run_exec_flow` in `crates/horch-core/src/harness/launch.rs:635-648`). While `claude::exec_command` and `codex::exec_command` both install the `Stop` hook on exec attempts, `antigravity::exec_command` and `run_exec_flow` currently do not call `seed_stop_hook`. This ticket closes that remaining exec-mode gap of R3-F2.

## Goal

An Antigravity exec-mode push attempt (`antigravity::exec_command` / `run_exec_flow`) seeds the `horch-result-gate` `Stop` hook in `<workdir>/.agents/hooks.json` using `seed_stop_hook` (preserving tracked files, refusing symlinks, and adding `/.agents/hooks.json` to `.git/info/exclude`), matching interactive Antigravity push launches and Claude/Codex exec launches.

## Requirements

- **R3-F2 (Important — FM-6)**:
  - **Finding text and file:line**:
    `crates/horch-core/src/harness/antigravity.rs:73` (now `:169`) builds the interactive Antigravity command, and `crates/horch-core/src/harness/antigravity.rs:148` (now `:247`) builds its exec command; before T-16 neither installed a Stop hook, and after T-16 (`b1382d1a`, `df3f9299`) only `Antigravity::command` (`:205-207`) calls `seed_stop_hook`, while `antigravity::exec_command` (`:247`) and `run_exec_flow` (`crates/horch-core/src/harness/launch.rs:635-648`) still do not. An Antigravity exec worker (the default worker mode under D7 and `design.md:934`) can therefore finish without the `.agents/hooks.json` `horch result --check` Stop hook gate required by FM-6.
  - **Design rules**:
    - **FM-6** (`docs/specs/fleetd/design.md:1551`): "Stop gate refuses to end the turn without `result.json` or a question file; exec mode exit without result triggers one repair resume; the next attempt reuses the same worktree."
    - **How each adapter applies a profile — `result_gate`** (`docs/specs/fleetd/design.md:935`): for Antigravity CLI, "`Stop` hook, with the same rule; plus the wrapper's repair pass in every case, which also accepts a question file in place of `result.json`."

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Write the failing test first in `crates/horch-core/tests/p7_antigravity_exec.rs`:
   - Add test function `agy_exec_launch_seeds_stop_hook_in_worktree`:
     - Create a temporary git worktree directory (`git init -q` and an empty initial commit) with an existing untracked `.agents/hooks.json` entry (such as `{"mine": {"Stop": [{"type": "command", "command": "mine"}]}}`).
     - Build or prepare the Antigravity exec command for that worktree (passing the `horch` executable path or calling `antigravity::seed_stop_hook` via `exec_command` / `ExecLaunch`).
     - Assert that `<workdir>/.agents/hooks.json` contains both `"mine"` and `"horch-result-gate"` whose `Stop` command runs `<horch> result --check` and maps exit code `2` to `{"decision":"continue"}`, and that `git status --porcelain --untracked-files=all` in the worktree is empty because `/.agents/hooks.json` is added to `.git/info/exclude`.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test p7_antigravity_exec -- agy_exec_launch_seeds_stop_hook_in_worktree` and see it fail before the fix is in place.
2. In `crates/horch-core/src/harness/antigravity.rs` and `crates/horch-core/src/harness/launch.rs`:
   - Expose `pub(crate) fn seed_stop_hook(worktree: &Path, horch_exe: &Path) -> Result<()>` in `crates/horch-core/src/harness/antigravity.rs` (or add `horch_exe: Option<&'a Path>` to `ExecLaunch<'a>` / call `seed_stop_hook` when `launch.workdir` exists and is a directory, or call `super::antigravity::seed_stop_hook(&workdir, &env.horch_exe)?` in `run_exec_flow` at `crates/horch-core/src/harness/launch.rs:640` and in `exec_command` when `workdir` is an existing directory).
   - If `exec_command` itself seeds the hook when `launch.workdir.is_dir()`, use a default or optional `horch_exe` on `ExecLaunch` (or resolve `horch` when not provided) so existing callers of `ExecLaunch` with a non-existent path like `/fleet/worktrees/T1` in `agy_exec_launch_has_the_flags_of_the_design` continue to work without error while real worktrees get `.agents/hooks.json` seeded, and `run_exec_flow` in `crates/horch-core/src/harness/launch.rs:640` passes `&env.horch_exe` and propagates any `seed_stop_hook` error.
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.
- Changing `crates/horch-core/src/harness/claude.rs` or `crates/horch-core/src/harness/codex.rs`.

## Done when

`agy_exec_launch_seeds_stop_hook_in_worktree` in `crates/horch-core/tests/p7_antigravity_exec.rs` passes, both interactive and exec Antigravity push launches seed `.agents/hooks.json` with the `horch result --check` Stop hook, and the ticket Check passes.
