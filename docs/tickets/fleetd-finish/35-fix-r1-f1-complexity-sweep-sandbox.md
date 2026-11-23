# 35: Fix R1-F1: run complexity-sweep in the worker sandbox profile (SR-4, SR-6)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test check_sandbox && scripts/dev-sandbox.sh cargo test -p horch-core --test verifier_review && scripts/dev-sandbox.sh cargo test -p horch-core --test w3_finish_steps && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/taskledger/finish/complexity.rs`, `crates/horch-core/tests/check_sandbox.rs`, `crates/horch-core/tests/verifier_review.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p0-pp-p1.md` (finding R1-F1) and `docs/specs/fleetd/design.md` (rules SR-4 and SR-6).

## Goal

The `task_check` job run for `complexity-sweep` executes `cargo clippy` and the static analyser inside the task worker's sandbox profile through `SandboxProfiles::check_launch` (with the operator denials on `<fleet root>/run`), never with the rights of fleetd or the job process. A worker commit with a project-controlled `build.rs` cannot inspect or write `<fleet root>/run/jobs/<id>-<run>.json` during `complexity-sweep`.

## Requirements

- **R1-F1 (Critical — SR-4, SR-6)**:
  - **Finding text and file:line**:
    `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:1132` (`fn complexity_sweep`) runs the complexity-sweep task check in the job process, explicitly with that process's rights, instead of using the worker's sandbox (`crates/horch-core/src/taskledger/finish/complexity.rs:286`, `:309`, `:373`, `:425`). `crates/horch-core/src/fleetd/fleet_runtime/adapters/job_result_file.rs:121` accepts a result file by path containment, same-user ownership and size, and `:159` only binds the payload to the job id and run in its filename. During complexity-sweep, a worker commit that makes `cargo clippy` execute a project-controlled build script has fleetd's access to `<fleet root>/run/jobs`, can inspect job filenames, and can write a forged passing result for another running gate job.
  - **Design rules**:
    - **SR-4** (`docs/specs/fleetd/design.md:180`): "An acceptance command shall run in the sandbox profile of the task's worker, never with the rights of fleetd."
    - **SR-6** (`docs/specs/fleetd/design.md:183`): "fleetd shall not trust a file that a worker writes. It shall read a file only when it is a regular file, opened without following a link, under the size limit of its type, with its real path inside its own attempt folder, and with the task id and attempt id of its path. A file that fails is recorded as rejected and is not read again."

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

1. Write the failing test first in `crates/horch-core/tests/check_sandbox.rs`:
   - Add test function `complexity_sweep_build_script_cannot_forge_another_running_job_result`:
     - Set up a `World` with a task on a git repository in its worktree that changes a Rust source file (`src/lib.rs`) and has a `build.rs` (or a fake `cargo` / analyser script invoked during `complexity-sweep`) that attempts to write a passing `JobResult` JSON payload to `<fleet root>/run/jobs/<other_job_id>-1.json` for another running gate job (`Kind::FastGate` or `Kind::FullGate`) in the `jobs` table.
     - Queue and run the `Kind::TaskCheck` job with a complexity-sweep `input_key` (`review::is_complexity_key`) using `w.host_sandboxes()` (`WorkerSandboxes`).
     - Assert that on a host with a sandbox (`host_gap().is_none()`), the write to `<fleet root>/run/jobs/<other_job_id>-1.json` is denied by the worker sandbox, no result file is created for the other running job, and `jobs::end_from_files` leaves the other running gate job in `State::Running`.
     - Also assert that when a task's worker has no sandbox profile (`NoProfiles`), `complexity-sweep` refuses to run with `"SR-4: ..."` and `verdict: false`, matching `task_check`.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test check_sandbox -- complexity_sweep_build_script_cannot_forge_another_running_job_result` and see it fail because `complexity_sweep` currently bypasses `SandboxProfiles::check_launch`.
2. In `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/taskledger/finish/mod.rs`, and `crates/horch-core/src/taskledger/finish/complexity.rs`:
   - Pass `checks: &CheckRunner<'_>` into `complexity_sweep` at `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:737` and `:1138`.
   - Require a worker and a sandbox profile from `checks.profiles.check_launch(&worker, dir, ...)` before executing `cargo clippy` or the complexity analyser. If the task has no worker or `check_launch` returns `None`, return `no_verdict` with `"SR-4: ... a check never runs with the rights of fleetd"` (same as `task_check` at `:1106-1120`).
   - Route the child command executions of `complexity::step` (`analyse_rust` at `crates/horch-core/src/taskledger/finish/complexity.rs:309` and `Analyser::run` at `:420`) through `checks.profiles.check_launch` and `checks.host` (`run_check`), writing the stdout JSON report inside a worker-writable path (such as inside `dir` or the worker's attempt folder) or capturing stdout from the sandboxed launch before copying it to `evidence_dir`, so the child process runs inside the worker's sandbox with the `OperatorPaths` denials (`<fleet root>/run`, token, and channel denied).
   - Update `crates/horch-core/tests/verifier_review.rs` (`run_complexity` at line 401) if needed so its `CheckRunner` provides a valid `SandboxProfiles` implementation in tests.
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.
- Changing `triage_apply` fact ingestion in `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:881` (covered by ticket 39 for R2-F1).

## Done when

`complexity_sweep_build_script_cannot_forge_another_running_job_result` in `crates/horch-core/tests/check_sandbox.rs` passes, `complexity-sweep` runs through `SandboxProfiles::check_launch` with the worker's sandbox and operator denials, and the ticket Check passes.
