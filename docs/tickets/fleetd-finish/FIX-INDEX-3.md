# Fix tickets index: end review R3 (Phase 4)

Review covered:
- R3: `docs/specs/fleetd/reviews/end-review-p4.md` (commit `5cc44579` on `fleet/fleetd-finish`)

## Findings covered by existing tickets or already fixed on `fleet/fleetd-finish`

### R3-F1 — Critical — SR-4 — `complexity-sweep` executes worker-controlled code with fleetd rights
- **Covered by existing ticket**: [35-fix-r1-f1-complexity-sweep-sandbox.md](35-fix-r1-f1-complexity-sweep-sandbox.md) (in [FIX-INDEX-1.md](FIX-INDEX-1.md), task T-37).
- **Why no new ticket is written**: Per the extra rule of `fix-tickets-brief-3.md`, R3-F1 is the same defect at `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:1132` and `:1147` as R1-F1. Ticket 35 routes both `cargo clippy` (`analyse_rust` at `crates/horch-core/src/taskledger/finish/complexity.rs:309`) and the complexity analyzer (`Analyser::run` at `:420`) through `checks.profiles.check_launch` and `checks.host`, refuses the job with `"SR-4: ..."` when the task has no worker or `check_launch` returns `None`, and adds the regression test `complexity_sweep_build_script_cannot_forge_another_running_job_result` in `crates/horch-core/tests/check_sandbox.rs` proving that worker-controlled code executed during `complexity-sweep` cannot write outside the worker's allowed paths. R3 names no case that ticket 35 does not cover.

### R3-F2 — Important — FM-6 — Antigravity has no Stop hook (interactive part fixed on `fleet/fleetd-finish`; exec part in ticket 60)
- **What is already fixed on `fleet/fleetd-finish` (task T-16 / ticket `16-d1-antigravity-stop-hook.md`, commits `b1382d1a`, `2f21c38b`, `df3f9299`)**:
  - **Code proof**: `crates/horch-core/src/harness/antigravity.rs:73-159` defines `HOOKS_FILE` (`".agents/hooks.json"`), `STOP_HOOK_NAME` (`"horch-result-gate"`), `stop_hook_command(horch_exe: &Path)` (running `stop_gate::hook_command(horch_exe)` and mapping exit status `2` of `horch result --check` to `{"decision":"continue","reason":...}`), `seed_stop_hook(worktree: &Path, horch_exe: &Path)` (skipping tracked `.agents/hooks.json`, refusing symlinks at `.agents` or `.agents/hooks.json`, adding `/.agents/hooks.json` to `<git-common-dir>/info/exclude`, and merging `horch-result-gate` into `<worktree>/.agents/hooks.json`), and `Antigravity::command` at `crates/horch-core/src/harness/antigravity.rs:205-207` calls `seed_stop_hook(worktree, &env.horch_exe)?` whenever `(&env.task_dir, &env.worktree)` are `Some`.
  - **Test proof**:
    - `crates/horch-core/src/harness/antigravity.rs:569`: `harness::antigravity::tests::the_stop_hook_maps_exit_2_to_continue`
    - `crates/horch-core/tests/harness_defaults.rs:873`: `antigravity_push_launch_has_a_stop_hook_that_runs_result_check`
    - `crates/horch-core/tests/harness_defaults.rs:899`: `antigravity_push_launch_keeps_a_tracked_hooks_file`
    - `crates/horch-core/tests/harness_defaults.rs:918`: `antigravity_push_launch_writes_through_no_symlink`
    - `crates/horch-core/tests/harness_defaults.rs:938`: `claim_attempt_has_no_stop_hook`
- **What T-16 did not cover (ticket 60 below)**: R3-F2 also cites the exec-mode command builder (`crates/horch-core/src/harness/antigravity.rs:148` at review commit `5cc44579`, now line `247`, `pub fn exec_command(launch: &ExecLaunch<'_>) -> Command`, called from `run_exec_flow` at `crates/horch-core/src/harness/launch.rs:635-648`). Unlike `claude::exec_command` and `codex::exec_command`, the Antigravity exec path still does not call `seed_stop_hook`. Ticket `60-fix-r3-f2-antigravity-exec-stop-hook.md` closes that remaining exec-mode gap.

### R3-F3 — Important — FR-4 — Planned task kinds disappear from `task.yaml`
- **Covered by existing tickets**: [53-fix-r5-f6-direct-judge-output-ticket-and-kind.md](53-fix-r5-f6-direct-judge-output-ticket-and-kind.md) and [51-fix-r5-f3-judge-verification-plan-no-command-check.md](51-fix-r5-f3-judge-verification-plan-no-command-check.md) (in [FIX-INDEX-2.md](FIX-INDEX-2.md)).
- **Why no new finding ticket is written**: R3-F3 reports that `crates/horch-core/src/fleetd/task_execution/application/planned_tasks.rs:339` stores new plan kinds (`judge`, `research`, `design`, `creative`) only in `spec_json` with `work_kind: None`, and `dispatch::facts` at `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:583` reads only `task.work_kind`, rendering `kind: null` in `task.yaml`. Ticket 53 (`53-fix-r5-f6-direct-judge-output-ticket-and-kind.md`, step 3) already owns `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs` and updates `dispatch::facts` (line 583) to resolve `kind` via `SELECT COALESCE(work_kind, json_extract(spec_json, '$.kind')) FROM tasks WHERE task_id = ?1` (matching `verifier::next_of` at `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:340`), and ticket 51 (`51-fix-r5-f3-judge-verification-plan-no-command-check.md`) passes the effective task kind into `verification::plan` in `verifier.rs`.
- **What ticket 53 does not cover**: Ticket 53's test (`dispatched_judge_task_renders_kind_judge_and_verdict_instructions_in_task_files` in `crates/horch-core/tests/task_files_p6.rs`) tests only a `--direct` `judge` task, whereas R3-F3 and R3's `## Tests that do not prove their rule` (bullet 4) also call for proving that planned non-legacy kinds (`judge`, `research`, `design`, `creative`) committed through `planned_tasks::add_planned_tasks` survive dispatch into `task.yaml`. That test is added in ticket `61-fix-r3-tests-push-dispatch-verifier-and-planned-kinds.md` (in `crates/horch-core/tests/task_files.rs`, avoiding any `Own` overlap with ticket 53).

## Fix tickets

| File | Finding ID | Rank | Persona | Own paths | Blocked by |
|---|---|---|---|---|---|
| [60-fix-r3-f2-antigravity-exec-stop-hook.md](60-fix-r3-f2-antigravity-exec-stop-hook.md) | R3-F2 (exec-mode remainder) | Important | primary `opus`, fallback `codex-sol` | `crates/horch-core/src/harness/antigravity.rs`, `crates/horch-core/src/harness/launch.rs`, `crates/horch-core/tests/p7_antigravity_exec.rs` |  |
| [61-fix-r3-tests-push-dispatch-verifier-and-planned-kinds.md](61-fix-r3-tests-push-dispatch-verifier-and-planned-kinds.md) | R3 weak tests (T-601, R3-F3) | Important (test proof) | primary `qa-engineer`, fallback `opus` | `crates/horch-core/tests/task_files.rs`, `crates/horch-e2e/tests/p4_verifier.rs` | Ticket 53 (`53-fix-r5-f6-direct-judge-output-ticket-and-kind.md`) and T-8 (`08-c5-land-t561.md`) |

## File overlaps and `pass.rs` avoidance

- **`pass.rs` avoidance**: Neither ticket 60 nor ticket 61 includes `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` in `**Own:**`. Neither is blocked by the landing chain (T-15).
- **No shared `Own` paths with `FIX-INDEX-1.md` or `FIX-INDEX-2.md`**:
  - Ticket 60 owns `crates/horch-core/src/harness/antigravity.rs` (already landed in T-16 on `fleet/fleetd-finish`), `crates/horch-core/src/harness/launch.rs`, and `crates/horch-core/tests/p7_antigravity_exec.rs`.
  - Ticket 61 owns `crates/horch-core/tests/task_files.rs` and `crates/horch-e2e/tests/p4_verifier.rs` (disjoint from ticket 53's `crates/horch-core/tests/task_files_p6.rs` and `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`).
- **Shared file notice across findings**:
  - R3-F1 and R1-F1 both target `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` and `crates/horch-core/src/taskledger/finish/complexity.rs`; handled in 1 ticket (`35-fix-r1-f1-complexity-sweep-sandbox.md`).
  - R3-F3 and R5-F6 both target `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`; handled in 1 finding ticket (`53-fix-r5-f6-direct-judge-output-ticket-and-kind.md`), with ticket 61 adding the multi-kind dispatch test in `crates/horch-core/tests/task_files.rs` after ticket 53 lands.

## Tests that do not prove their rule

1. **SR-4 complexity-sweep outside-path test**: Covered by finding ticket [35-fix-r1-f1-complexity-sweep-sandbox.md](35-fix-r1-f1-complexity-sweep-sandbox.md) (task T-37), which adds `complexity_sweep_build_script_cannot_forge_another_running_job_result` in `crates/horch-core/tests/check_sandbox.rs`.
2. **FM-6 Antigravity Stop hook adapter test (`crates/horch-core/tests/harness_defaults.rs:761`)**: Interactive push and claim launches are now proven on `fleet/fleetd-finish` by T-16 (`antigravity_push_launch_has_a_stop_hook_that_runs_result_check`, `antigravity_push_launch_keeps_a_tracked_hooks_file`, `antigravity_push_launch_writes_through_no_symlink`, `claim_attempt_has_no_stop_hook` at `crates/horch-core/tests/harness_defaults.rs:873-944`); exec-mode push launches are covered by finding ticket [60-fix-r3-f2-antigravity-exec-stop-hook.md](60-fix-r3-f2-antigravity-exec-stop-hook.md) (`agy_exec_launch_seeds_stop_hook_in_worktree` in `crates/horch-core/tests/p7_antigravity_exec.rs`).
3. **T-601 push-dispatch-to-verifier chain (`crates/horch-e2e/tests/p4_verifier.rs:101`)**: Covered by [61-fix-r3-tests-push-dispatch-verifier-and-planned-kinds.md](61-fix-r3-tests-push-dispatch-verifier-and-planned-kinds.md) (`push_dispatched_task_launches_writes_task_files_and_reaches_done_through_verifier` in `crates/horch-e2e/tests/p4_verifier.rs`).
4. **Planned non-legacy kinds in `task.yaml` (`crates/horch-core/tests/task_files.rs:17` and `crates/horch-core/tests/task_files_p6.rs:19`)**: Covered by [53-fix-r5-f6-direct-judge-output-ticket-and-kind.md](53-fix-r5-f6-direct-judge-output-ticket-and-kind.md) (for `--direct` `judge` tasks in `crates/horch-core/tests/task_files_p6.rs`) and [61-fix-r3-tests-push-dispatch-verifier-and-planned-kinds.md](61-fix-r3-tests-push-dispatch-verifier-and-planned-kinds.md) (`planned_non_legacy_kinds_survive_dispatch_into_task_yaml` for `judge`, `research`, `design`, and `creative` in `crates/horch-core/tests/task_files.rs`).
5. **SR-3 external messaging boundary (`crates/horch-core/tests/sr3_no_agent_push.rs:122`)**: Covered by existing run ticket [17-d2-sr3-external-message.md](17-d2-sr3-external-message.md) (unit D2 in `MAP.md`, primary `qa-engineer`, fallback `opus`, owning `crates/horch-core/src/fleetd/worker_profiles/domain/launch_env.rs`, `crates/horch-core/tests/sr3_no_agent_push.rs`, `crates/horch-e2e/tests/p4_safety.rs`, and `crates/horch-platform/src/sandbox.rs`).

## Minor findings

- **R3 (`end-review-p4.md`)**: None (0 Minor findings).

## Open questions

1. **R3 operator decision 1 — Antigravity Phase 4 adapter support**: Decide whether Antigravity remains a supported Phase 4 adapter. Recommendation: keep it supported only after adding a trusted Stop hook; otherwise remove it from the Phase 4 support claim and keep FM-6 non-green for that adapter. (Note: T-16 landed the interactive Stop hook on `fleet/fleetd-finish`, and ticket 60 adds it to the exec launch path.)
2. **R3 operator decision 2 — Exact boundary of SR-3's prohibition on external messages**: Decide the exact boundary of SR-3's prohibition on external messages. Recommendation: define the prohibited destinations and transports, then add adapter tests for that finite contract; the current git-hook tests prove only the git operations they name. (Note: ticket `17-d2-sr3-external-message.md` tests and enforces `gh issue comment`, `gh pr comment`, `gh pr review`, and outbound webhook `curl` denials across worker profiles and the verifier check sandbox.)
