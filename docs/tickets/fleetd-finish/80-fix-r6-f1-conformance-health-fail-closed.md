# 80: Fix R6-F1: process conformance before dispatch and fail closed on missing or invalid conformance.json (FR-8, FM-19, SR-8)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_pass_p7 && scripts/dev-sandbox.sh cargo test -p horch-core --test p7_profile_health && scripts/dev-sandbox.sh cargo test -p horch-core --test p7_docs_jobs && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/health.rs`, `crates/horch-core/tests/fleetd_pass_p7.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p7.md` (finding R6-F1) and `docs/specs/fleetd/design.md` (rules FR-8, FM-19, SR-8).

## Goal

`Pass::run` observes ended jobs and processes Phase 7 conformance results before `dispatch_rows` runs, holds profiles of a changed CLI until its current conformance report passes, and marks profiles of that CLI unhealthy when an ended conformance job produces a missing (`Ok(None)`) or unreadable/invalid (`Err`) `conformance.json` instead of recording `CONFORMANCE_ACTED` without a health mark.

## Requirements

- **R6-F1 (Critical — FR-8, FM-19, SR-8)**:
  - **Finding text and file:line**:
    FR-8, FM-19 and the SR-8 Operator boundary require dispatch to use current conformance health. The pass dispatches at `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:1313` and does not process ended phase-7 jobs until `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:1325`. A profile can therefore receive 1 task in the pass that first observes its failed conformance result.
    The failure persists for a missing or malformed report. `act_on_conformance` writes no health mark for `Ok(None)` or `Err` at `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4250`, but it records `CONFORMANCE_ACTED` for both at `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4260`. `conformance_report` returns `None` when the result has no path and returns an error for unreadable or invalid JSON at `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4434`. The scheduler has no immediate retry after that conformance row at `crates/horch-core/src/fleetd/worker_profiles/domain/docs_jobs.rs:135`.
    Concrete case: a changed Codex binary writes no usable `conformance.json`. A ready Codex task binds before the result is applied, and later passes continue to treat the profile as healthy. If its sandbox boundary changed, the worker can receive Operator rights. The smallest fix is to process conformance before dispatch, hold a changed profile until a current report passes, and mark an invalid or missing report unhealthy instead of recording it as acted without a mark.
  - **Design rules**:
    - **FR-8** (`docs/specs/fleetd/design.md:144`): "A docs-sync job shall fetch each CLI's documentation, record the installed version, and run adapter conformance tests on change."
    - **FM-19** (`docs/specs/fleetd/design.md:1564`): "CLI update changes a flag or default | Every worker of that CLI fails | Docs sync plus conformance on version change; failing CLI marked unhealthy | Before dispatch | FR-8."
    - **SR-8** (`docs/specs/fleetd/design.md:182`): "The sandbox of each worker shall deny reading the token file, connecting to the command channel and writing into the request folder, and profile validation shall refuse a profile whose sandbox allows 1 of them."

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

## Steps

1. Write the failing tests first in `crates/horch-core/tests/fleetd_pass_p7.rs`:
   - Add test function `failed_conformance_blocks_dispatch_in_the_same_pass`:
     - Set up a `World` with `"claude"` installed (`w.install_cli("claude", "2.1.0")`), run 1 pass so `conformance` is queued and started, then add a ready task `t1` for `CLAUDE_PROFILE` (`w.ready_task("t1", CLAUDE_PROFILE)`) and end the conformance job with a failing report (`w.end_conformance(id, run, &report("claude", true))`).
     - Run the next pass (`run_pass(&w, &fake)`).
     - Assert that `w.health_events(CLAUDE_PROFILE)` is `["profile_unhealthy"]` and `w.status(&t1)` stays `"ready"` (the failed conformance result is processed before dispatch in that same pass, so `t1` is never bound to an attempt).
   - Add test function `changed_cli_and_missing_or_corrupt_conformance_report_fail_closed`:
     - Set up a `World` where `"claude"` `"2.1.0"` has passed conformance, then update the binary (`w.install_cli("claude", "2.2.0")`) and add a ready task `t1` for `CLAUDE_PROFILE`.
     - Run 1 pass where the changed CLI binary is scanned and queues its new `conformance` job (`Why::NewCli`), and assert that `t1` is held (`w.status(&t1) == "ready"`) while the changed CLI's conformance report is pending.
     - End the new conformance job first with a `JobResult` that has `data: None` (missing report path, e.g. version command failed) or a corrupt `conformance.json` file at `path`.
     - Run a pass and assert that `CLAUDE_PROFILE` is marked unhealthy (`w.health_events(CLAUDE_PROFILE)` ends with `"profile_unhealthy"`) and `t1` remains `"ready"` on that pass and subsequent passes until a valid passing conformance report is observed.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_pass_p7 -- failed_conformance_blocks_dispatch_in_the_same_pass` and see it fail because `dispatch_rows` currently runs at `pass.rs:1313` before `jobs::observe` (`:1320`) and `p7_rows` (`:1325`).
2. Fix conformance ordering, changed-profile holding, and fail-closed report handling in `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` and `crates/horch-core/src/fleetd/worker_profiles/application/health.rs`:
   - In `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`:
     - Move `let ctx = self.job_ctx(); let observed = jobs::observe(&ctx, &tx, &read.jobs, clock)?;` and `self.p7_rows(&tx, &read.p7, read.pools.as_ref(), clock)?` ahead of the `if let Some(w) = &self.work` dispatch block (currently at line 1307), so ended `conformance` jobs update profile health and newly scanned CLI binary changes are processed before `dispatch_rows` reads `health::unhealthy_profiles(tx)`.
     - When `queue_docs_jobs` queues a `Kind::Conformance` job with `Why::NewCli` for a CLI (or in `p7_rows` when a scanned CLI triggers a new-binary conformance run), hold or mark the profiles of that CLI's `exec_harness` unhealthy (or pending conformance) until an ended `conformance` run for that binary id passes.
     - In `act_on_conformance` (`pass.rs:4241-4274`): when `conformance_report(&row)` returns `Ok(None)` or `Err(e)`, do not leave the profiles of that job's CLI healthy. Extract the CLI name from `row.subject.split_whitespace().next()` and mark every matching profile in `profiles` (`exec_harness(cli) == Some(agent)`) unhealthy via `health::mark_cli_unhealthy(tx, guard, cli, &reason, profiles)` with a clear reason (for `Ok(None)`: `&row.result_json` / `"conformance run <id> produced no report"`; for `Err(e)`: `"conformance run <id> produced an unreadable or invalid report: <e>"`), and open the `adapter-conformance-failed` needs-you item / drift note as appropriate so the failure is visible and fails closed.
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs` (covered by ticket 81 for R6-F2).
- Changing non-Phase-7 job observation semantics.

## Done when

`failed_conformance_blocks_dispatch_in_the_same_pass` and `changed_cli_and_missing_or_corrupt_conformance_report_fail_closed` in `crates/horch-core/tests/fleetd_pass_p7.rs` pass, conformance health is applied before dispatch in the same pass, missing or malformed `conformance.json` marks the CLI's profiles unhealthy, and the ticket Check passes.
