# CI FIX-10: fix unexplained failures in horch-core baseline_oracles.rs, fr5_push_only.rs, harness.rs, intake_roster.rs, p5_profile_validation.rs, p7_wrapper_exec.rs and trf_02_ticket_template.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test baseline_oracles --test fr5_push_only --test harness --test intake_roster --test p5_profile_validation --test p7_wrapper_exec --test trf_02_ticket_template --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/baseline_oracles.rs`, `crates/horch-core/tests/fr5_push_only.rs`, `crates/horch-core/tests/harness.rs`, `crates/horch-core/tests/intake_roster.rs`, `crates/horch-core/tests/p5_profile_validation.rs`, `crates/horch-core/tests/p7_wrapper_exec.rs`, `crates/horch-core/tests/trf_02_ticket_template.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

Run each of the 8 class `f?` tests in `crates/horch-core/tests/baseline_oracles.rs` (1 test), `crates/horch-core/tests/fr5_push_only.rs` (1 test), `crates/horch-core/tests/harness.rs` (1 test), `crates/horch-core/tests/intake_roster.rs` (2 tests), `crates/horch-core/tests/p5_profile_validation.rs` (1 test), `crates/horch-core/tests/p7_wrapper_exec.rs` (1 test), and `crates/horch-core/tests/trf_02_ticket_template.rs` (1 test) 3 times locally through `scripts/dev-sandbox.sh`, find the cause of each failure, and fix it.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- The 7 test files in `**Own:**`.

## Tests covered (8 class `f?` rows)

1. `crates/horch-core/tests/baseline_oracles.rs` (1 test):
   - `oracle_launch_matches` (`crates/horch-core/tests/baseline_oracles.rs:57`, Linux): `launch/architect-reviewer.json differs from the A0 oracle at line 31:`
2. `crates/horch-core/tests/fr5_push_only.rs` (1 test):
   - `only_the_bind_and_its_dispatch_points_choose_a_task` (`crates/horch-core/tests/fr5_push_only.rs:136`, Linux + macOS): `FR-5: only the bind module, fleetd and horch spawn choose a task:`
3. `crates/horch-core/tests/harness.rs` (1 test):
   - `harness_codex_rules_snapshot` (`crates/horch-core/tests/harness.rs:25`, Linux + macOS): `assert_eq failed: codex_rules_orchestrator.rules differs from the pre-A4 snapshot`
4. `crates/horch-core/tests/intake_roster.rs` (2 tests):
   - `t27_every_intake_teammate_reads_but_does_not_write_its_bundle` (`crates/horch-core/tests/intake_roster.rs:454`, Linux + macOS): `assert_eq failed: intake-auditor`
   - `t28_a_full_sandboxed_teammate_writes_no_skills_bundle` (`crates/horch-core/tests/intake_roster.rs:506`, Linux + macOS): `skill-importer`
5. `crates/horch-core/tests/p5_profile_validation.rs` (1 test):
   - `builtin_roster_has_no_refused_worker_profile_on_macos` (`crates/horch-core/tests/p5_profile_validation.rs:356`, Linux + macOS): `refused worker profiles:`
6. `crates/horch-core/tests/p7_wrapper_exec.rs` (1 test):
   - `interactive_attempt_is_unchanged` (`crates/horch-core/tests/p7_wrapper_exec.rs:457`, Linux + macOS): `assert_eq failed: claude`
7. `crates/horch-core/tests/trf_02_ticket_template.rs` (1 test):
   - `trf_02_template_example_map_parses_with_the_ticket` (`crates/horch-core/tests/trf_02_ticket_template.rs:16`, Linux + macOS): `the template has no '== Example MAP.md ==' line`

## Steps

1. Run each of the 8 tests locally through `scripts/dev-sandbox.sh` and inspect the failure:
   - `crates/horch-core/tests/baseline_oracles.rs:57` (`oracle_launch_matches`): check why `launch_oracle` differed on Linux (where `bwrap` and `socat` were absent on CI or where the platform sandbox / 256k context / plugin path differed).
   - `crates/horch-core/tests/fr5_push_only.rs:136` (`only_the_bind_and_its_dispatch_points_choose_a_task`): Phase 6 retired `horch spawn` (`crates/horch/src/cmd/taskledger_spawn.rs`), so `choosers.iter().any(|(f, _)| f == SPAWN)` at line 142 fails or `SPAWN` is no longer a caller of `choices`. Update the assertion in `fr5_push_only.rs` to match Phase 6 (where only the bind module and fleetd dispatch choose a task).
   - `crates/horch-core/tests/harness.rs:25` (`harness_codex_rules_snapshot`): inspect the diff in `codex_rules_orchestrator.rules` after Phase 6 retired `horch spawn` / `horch ledger run new` from the orchestrator exec rules, and update the snapshot check in `harness.rs` (or if the snapshot file is in `crates/horch-core/tests/snapshots/`, report if an extra `Own` path is needed).
   - `crates/horch-core/tests/intake_roster.rs:454` (`t27_every_intake_teammate_reads_but_does_not_write_its_bundle`) and `:506` (`t28_a_full_sandboxed_teammate_writes_no_skills_bundle`): inspect why `--settings` permissions for `intake-auditor` and `skill-importer` changed in Phase 5/7 (such as operator boundary deny rules or skill bundle paths) and update the test assertions in `intake_roster.rs` without weakening the rule that the teammate cannot write its bundle.
   - `crates/horch-core/tests/p5_profile_validation.rs:356` (`builtin_roster_has_no_refused_worker_profile_on_macos`): inspect which profiles in `Roster::builtin()` are refused (for example Phase 6 sub-orchestrator / intake / verifier profiles that are not worker profiles or Linux `bwrap`/`socat` host checks) and update `p5_profile_validation.rs`.
   - `crates/horch-core/tests/p7_wrapper_exec.rs:457` (`interactive_attempt_is_unchanged`): Phase 7 ticket 19 made `exec` the default mode when `teammate.mode` is `None` (`ExecCli::of(&teammate("claude", None)) == Some(ExecCli::Claude)` while `Some("interactive")` returns `None`). Update line 457 of `p7_wrapper_exec.rs` to match the `None` (default `exec`) vs `Some("interactive")` contract.
   - `crates/horch-core/tests/trf_02_ticket_template.rs:16` (`trf_02_template_example_map_parses_with_the_ticket`): Phase 6 replaced `== Example MAP.md ==` with `== Example plan.yaml ==` in `teammates/ticket-writer.md` (`docs/specs/ticket-format.md` section 4). Update `trf_02_ticket_template.rs` to parse the `== Example plan.yaml ==` block (or the example in `docs/specs/ticket-format.md`).
2. Never weaken an assert. Run each test 3 times through `scripts/dev-sandbox.sh`.
3. Run the Check.

## Out of scope

- Any file outside `**Own:**`.

## Done when

All 8 tests pass 3 times in a row, and the Check passes.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.
