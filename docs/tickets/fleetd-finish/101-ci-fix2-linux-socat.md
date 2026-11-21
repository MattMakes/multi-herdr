# CI FIX-2: install socat with bubblewrap on Linux CI

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo fmt --all --check`
**Own:** `.github/workflows/ci.yml`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Linux`, `### Groups of test (ubuntu-latest)` and `## Fix units` row FIX-2).

## Goal

On Linux CI (`test (ubuntu-latest)`), 27 tests of `horch-core` fail in class `(e)` with `refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat` (or `socat not on PATH; install bubblewrap and socat`). Install `socat` alongside `bubblewrap` in `.github/workflows/ci.yml` so the Linux platform sandbox check in `crates/horch-platform/src/linux/sandbox.rs` finds both tools on `PATH`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md` in your worktree: section `## Linux` and `## Fix units` row FIX-2.
- `.github/workflows/ci.yml`: the Linux step under job `test` (around lines 24 to 29).
- `crates/horch-platform/src/linux/sandbox.rs`: the host check that requires `bwrap` and `socat` on `PATH`.

## Tests covered (27 class `e` rows of the CI report)

- `adapter_hash_changes_when_the_rendered_launch_changes` (`crates/horch-core/tests/p7_conformance_job.rs:346`): `Err: conformance-claude: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `arc_09_argv_matches_baseline` (`crates/horch-core/tests/baseline_oracles.rs:498`): `assert_eq failed: architect-reviewer: adapter argv differs from the A0 oracle`
- `ctx_05_claude_overlay_gets_fleet_window_only_without_operator_value` (`crates/horch-core/tests/context_launch.rs:157`): `Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `ctx_05_recorded_applied_matches_the_command` (`crates/horch-core/tests/context_launch.rs:157`): `Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `execution::lifecycle::tests::arc_06_transport_env_applied_to_child` (`crates/horch-core/src/execution/lifecycle.rs:2106`): `Err: sonnet: refusing to launch without a sandbox: socat not on PATH; install bubblewrap and socat`
- `harness::claude::tests::worker_settings_deny_git_worktree_add_remove_and_prune` (`crates/horch-core/src/harness/claude.rs:2894`): `Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::a_haiku_override_launches_without_effort` (`crates/horch-core/src/harness/launch.rs:2275`): `Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::arc_29_launch_trusts_the_workdir_before_the_agent_starts` (`crates/horch-core/src/harness/launch.rs:3316`): `Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::claude_exec_launch_exposes_its_skills_as_a_plugin_dir` (`crates/horch-core/src/harness/launch.rs:3380`): `Err: exec-claude: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::claude_fresh_carries_session_and_mode` (`crates/horch-core/src/harness/launch.rs:1777`): `Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::claude_resume_swaps_the_session_flag` (`crates/horch-core/src/harness/launch.rs:1816`): `Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::declared_mcp_servers_are_strict_and_complete` (`crates/horch-core/src/harness/launch.rs:2811`): `Err: frontend-developer: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::disabled_skills_become_skill_overrides` (`crates/horch-core/src/harness/launch.rs:3014`): `Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::git_env_agent_launch_child` (`crates/horch-core/src/harness/launch.rs:3173`): `Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::operator_only_teammate_with_every_skill_missing_still_launches` (`crates/horch-core/src/harness/launch.rs:3228`): `Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::opting_in_to_claudeai_skills_leaves_the_switch_unset` (`crates/horch-core/src/harness/launch.rs:2969`): `Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::path_of_blender_fills_blender_path_or_leaves_it_untouched` (`crates/horch-core/src/harness/launch.rs:1612`): `Err: blender-artist: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::phase_skills_are_native_on_all_harnesses_and_reach_every_worker` (`crates/horch-core/src/harness/launch.rs:1657`): `Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::plugin_paths_are_expanded_before_launch` (`crates/horch-core/src/harness/launch.rs:2874`): `Err: staff-engineer: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::plugin_skills_load_a_filtered_copy_of_the_plugin` (`crates/horch-core/src/harness/launch.rs:2141`): `Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::restricted_settings_keep_the_operator_status_line` (`crates/horch-core/src/harness/launch.rs:3073`): `Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::unset_isolation_fields_add_no_flags` (`crates/horch-core/src/harness/launch.rs:2892`): `Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `harness::launch::tests::variadic_flags_are_fenced_off_from_the_prompt_with_double_dash` (`crates/horch-core/src/harness/launch.rs:2841`): `Err: frontend-developer: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `hdf_04_migrated_launch_env_and_args_match_w2` (`crates/horch-core/tests/harness_defaults.rs:82`): `Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `launch_environment_sets_256k_context_limit` (`crates/horch-core/tests/p7_context_256k.rs:79`): `Err: app-release-preparer: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `resume_keeps_an_override_and_refuses_a_hand_edited_record` (`crates/horch-core/tests/execution_plan.rs:1418`): `Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`
- `spawn_model_fable_is_allowed_for_an_ordinary_claude_teammate` (`crates/horch-core/tests/execution_plan.rs:1418`): `Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat`

## Steps

1. In `.github/workflows/ci.yml`, in the `if: runner.os == 'Linux'` step of the `test` job (lines 24 to 28), change `sudo apt-get install -y bubblewrap` to `sudo apt-get install -y bubblewrap socat` and update the comment above it to mention `bubblewrap` and `socat`.
2. Run the Check: `cargo fmt --all --check`.

## Out of scope

- Any Rust code or test file.
- Starting a GitHub Actions workflow run (the next CI run after FIX-1 through FIX-6 proves this on Linux).

## Done when

`.github/workflows/ci.yml` installs `bubblewrap socat` on Linux and `cargo fmt --all --check` passes.

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
