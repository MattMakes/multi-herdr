# End review of Phase 4: verifier, results, landing, gates, jobs, and git sandbox

## Summary

- Critical: 1 verified finding.
- Important: 2 verified findings.
- Minor: 0 findings.
- The review covered all 15 assigned watchlist lines; 12 have no additional production fault.

## Findings

### R3-F1 — Critical — SR-4 — complexity-sweep executes worker-controlled code with fleetd rights

**Status: verified.** `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:1132` starts the complexity-sweep job, and `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:1147` explicitly runs `cargo clippy` and the analyzer with the rights of the fleetd process. A worker can commit a malicious build script, procedural macro, or analyzer input before verification. The sweep then executes that worker-controlled code outside the worker sandbox, which gives it fleetd's file and network rights. This violates SR-4 and crosses the ticket's Critical threshold because a worker can gain operator rights.

Smallest fix: run the complete complexity-sweep command through the task worker's sandbox profile, as the task-check job does, and refuse the job when that sandbox cannot be created. Add a regression test whose complexity-sweep tries to write outside the worker's allowed paths.

### R3-F2 — Important — FM-6 — Antigravity has no Stop hook

**Status: verified.** `crates/horch-core/src/harness/antigravity.rs:73` builds the interactive Antigravity command, and `crates/horch-core/src/harness/antigravity.rs:148` builds its exec command; neither installs a Stop hook. The harness defaults tests cover only the Claude and Codex hooks at `crates/horch-core/tests/harness_defaults.rs:761`. An Antigravity worker can therefore finish without the trusted `horch result --check` gate required by FM-6 and depends on the repair path after exit instead of being stopped before the turn ends.

Smallest fix: install a per-attempt Antigravity Stop hook or plugin that invokes the trusted result check, and add the same adapter-level test used for Claude and Codex. If Antigravity cannot support this contract, remove it from the supported Phase 4 adapter set and revise the gate explicitly.

### R3-F3 — Important — FR-4 — planned task kinds disappear from task.yaml

**Status: verified.** `crates/horch-core/src/fleetd/task_execution/application/planned_tasks.rs:339` stores new plan kinds such as `judge`, `research`, `design`, and `creative` only in `spec_json`; their legacy `work_kind` is null. The verifier correctly falls back to `spec_json.kind` at `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:340`, but dispatch still assigns only `task.work_kind` to `TaskFacts.kind` at `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:583`. `crates/horch-core/src/fleetd/task_execution/domain/task_files.rs:61` then renders that missing value into `task.yaml`. The worker does not receive the kind required by FR-4 and can apply the wrong work and reporting contract.

Smallest fix: add one effective-kind reader that returns `COALESCE(work_kind, json_extract(spec_json, '$.kind'))`, use it in dispatch and verification, and add a dispatch-to-`task.yaml` test for a planned `judge` or `design` task.

## Watchlist lines covered

- Line 7, T-366 — checked, no fault. `crates/horch-core/src/fleetd/integration/adapters/git.rs:250` routes `git merge-tree` through `CliWorktreeGit::checked`; `crates/horch-core/src/worktree_custody/adapters/git.rs:664` applies the git sandbox and project-program scan. The merge-driver test at `crates/horch-core/tests/worktree_jobs.rs:1210` verifies that it cannot read a HOME secret or write outside its named paths.
- Line 8, T-345 — checked, no fault. `crates/horch-core/src/taskledger/schema.rs:1254` deterministically gives every eligible run one feature row and gives a duplicated branch only to the newest row. `crates/horch-core/tests/taskledger_schema_p4.rs:343` verifies that the older duplicate keeps a branchless feature and no branch rows while the newer duplicate owns the branch.
- Line 10, T-370 — checked, no fault. `crates/horch-core/src/fleetd/integration/adapters/git.rs:298` derives the bundle commit date internally and passes only `GIT_AUTHOR_DATE` and `GIT_COMMITTER_DATE` through `with_env`; no worker-controlled environment key or value reaches this API. `crates/horch-core/tests/bundles.rs:787` verifies that a crash and retry reuse the same candidate commit.
- Line 11, T-357 — finding R3-F2. `crates/horch-core/src/harness/antigravity.rs:73` and `crates/horch-core/src/harness/antigravity.rs:148` have no Stop hook.
- Line 13, ticket 30 — checked, no fault. `crates/horch-platform/src/sandbox.rs:120` exposes only the traversal parents needed to reach an allowed repository below HOME, while keeping sibling contents hidden. `crates/horch-core/tests/worktree_jobs.rs:1210` verifies that a job cannot read a HOME secret and can list only the permitted mount-parent path.
- Line 14, T-542, first `work_kind` line — finding R3-F3. `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:594` reads only the legacy nullable field.
- Line 23, T-363 — finding R3-F1. `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:1147` runs complexity-sweep with fleetd rights.
- Line 25, T-365 — checked, no fault. `crates/horch-core/src/fleetd/worker_profiles/domain/launch_env.rs:29` adds agent hooks without replacing the repository hooks, and `crates/horch-core/tests/sr3_no_agent_push.rs:122` verifies that all 26 repository hooks remain in the chain and protected pushes remain refused.
- Line 29, T-587 — checked, no fault. `crates/horch-core/src/taskledger/supervisor.rs:96` defines the closed `OutcomeHint` enum with only `quota` and `context_limit`. `crates/horch-core/tests/p7_wrapper_exec.rs:307` verifies that success-like forged words are rejected and do not create `result.json`.
- Line 30, second `work_kind` line — finding R3-F3. `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:340` has the required fallback, but dispatch does not share it.
- Line 32, T-883 — checked, no fault. `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs:44` validates the protected file identity, root, owner, mode, parents, and size; `crates/horch-core/src/fleetd/fleet_runtime/application/job_result_file.rs:139` binds payload job and run to the filename; `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:833` binds the result to the current running row. `crates/horch-core/src/fleetd/integration/application/exec_gate.rs:211` derives the gate record's feature and tips from the trusted job row, and invalid facts roll back before the job fails.
- Line 47, T-402 — checked, no fault. `crates/horch-e2e/tests/p4_release.rs:607` verifies that a pre-release verifying task lands once after release. `crates/horch-core/tests/bundles.rs:869` verifies that an already-contained task enters a bundle without a candidate commit, and `crates/horch-core/tests/landing_states.rs:545` rejects an unproved direct transition from building to landed.
- Line 48, T-601 — checked, no production fault; the proof gap remains. `crates/horch-e2e/tests/p4_verifier.rs:101` still constructs a stand-in process and manually binds the attempt instead of using the real push dispatch and launch path.
- Line 56, T-603 — checked, no fault. `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:299` intentionally replaces an exit outcome when the verifier reaches its decision. `crates/horch-core/tests/verifier_review.rs:547` verifies that an already-ended `no_result` attempt becomes `verified` after all verification steps pass.
- Line 58, T-894 — checked, no fault. `crates/horch-core/src/fleetd/integration/adapters/git.rs:466` gives git readers the selected repository below HOME, while writers receive only the resolved git directory. `crates/horch-core/src/fleetd/integration/application/commands.rs:381` verifies ref-tip reads for a project below HOME; the general HOME and runtime-directory hides remain active.

## Tests that do not prove their rule

- The SR-4 outside-path test exercises the normal task-check path, but it never executes the complexity-sweep path. It therefore cannot detect R3-F1.
- `crates/horch-core/tests/harness_defaults.rs:761` proves Stop hooks for Claude and Codex only. It does not prove FM-6 for Antigravity.
- `crates/horch-e2e/tests/p4_verifier.rs:101` uses a stand-in process and manual attempt binding. It proves result ingestion and verification after binding, but not the real push dispatch, launcher, task-file, and hook chain requested by T-601.
- `crates/horch-core/tests/task_files.rs:17` and `crates/horch-core/tests/task_files_p6.rs:19` manually construct `TaskFacts` with `kind: Some("feature")`. They do not prove that a planned non-legacy kind survives dispatch into `task.yaml`.
- `crates/horch-core/tests/sr3_no_agent_push.rs:122` proves repository-hook preservation and denial of protected pushes. It does not prove the full SR-3 phrase "message anyone outside the fleet" for every supported CLI or network client.

## Operator decisions

- Decide whether Antigravity remains a supported Phase 4 adapter. Recommendation: keep it supported only after adding a trusted Stop hook; otherwise remove it from the Phase 4 support claim and keep FM-6 non-green for that adapter.
- Decide the exact boundary of SR-3's prohibition on external messages. Recommendation: define the prohibited destinations and transports, then add adapter tests for that finite contract; the current git-hook tests prove only the git operations they name.
