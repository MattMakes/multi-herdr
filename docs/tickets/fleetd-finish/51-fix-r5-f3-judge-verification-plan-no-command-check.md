# 51: Fix R5-F3: pass task kind into verification::plan so a judge task skips the command check and runs its structural verdict check (RR-3, FM-9, FM-33)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib fleetd::task_execution::domain::verification && scripts/dev-sandbox.sh cargo test -p horch-core --test judge_pair && scripts/dev-sandbox.sh cargo test -p horch-core --test verifier && scripts/dev-sandbox.sh cargo test -p horch-core --test verifier_structural && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/verification.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/tests/judge_pair.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p6.md` (finding R5-F3) and `docs/specs/fleetd/design.md` (rules RR-3, FM-9, FM-33).

## Read first: overlap with landing unit C5 (T-561)

Ticket 08 (`08-c5-land-t561.md`, landing branch `origin/fleet/fleetd-build-T561-the-e2e-tests-of-the-judge-pair-rr-3-fm`) touches `crates/horch-core/src/fleetd/task_execution/domain/verification.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, and `crates/horch-core/tests/judge_pair.rs` (commits `0eafdc37` and `8a207cb6`, adding `JUDGE_NO_CHECK`, `plan_judge`, and `planned_judge_task_with_no_check_passes_on_its_verdict`). Check `git log` in your worktree first: if C5 (T-561) has already landed, build on its `JUDGE_NO_CHECK` and `planned_judge_task_with_no_check_passes_on_its_verdict` so that `Facts` and `verification::plan` carry the effective task kind directly and unit tests in `verification.rs` prove `verification::plan` on a changed `judge` task with no command check.

## Goal

`verification::Facts` and `verification::plan` in `crates/horch-core/src/fleetd/task_execution/domain/verification.rs` take the effective task kind (or judge role), and `verification::plan` decides `Decision::NotApplicable` with `JUDGE_NO_CHECK` (`"a judge task is decided by its structural check and its verdict"`) for a changed `judge` task that has no check command, so `verifier::verify_one` proceeds directly to the structural judge-verdict check instead of failing with `"no check"`.

## Requirements

- **R5-F3 (Important — RR-3, FM-9, FM-33)**:
  - **Finding text and file:line**:
    The plan schema deliberately permits a judge task without acceptance at `crates/horch-core/src/fleetd/ideas/domain/plan_validator.rs:168`, and direct plans create exactly that task at `crates/horch-core/src/fleetd/ideas/application/direct.rs:265`. Verification facts contain no task kind at `crates/horch-core/src/fleetd/task_execution/domain/verification.rs:67`; `verification::plan` rejects a changed task with no command and no `check_none` at lines 100-117. The later structural routing at `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:340` cannot repair an already failed command verdict. A direct docs or report task creates a judge that commits its verdict file. Because the judge has changes but no command check, fleetd records `no check` and never reaches the structural judge-verdict check. The judged task then waits for its judge. The smallest fix is to pass the effective task kind into `verification::plan` and make `judge` skip the command-check decision and proceed directly to its structural verdict check.
  - **Design rules**:
    - **RR-3** (`docs/specs/fleetd/design.md:155`): "No task shall enter `done` without a deterministic check result recorded by the verifier. For a judgment kind (research, design, docs, creative or report), `done` requires a structural check of the output and a pass verdict from a separate judge task, with no open CRITICAL item."
    - **FM-9** (`docs/specs/fleetd/design.md:1552`): a judgment task reaches `done` only after its paired judge task passes its structural verdict check.
    - **FM-33** (`docs/specs/fleetd/design.md:1578`): a judge task has no command check of its own; its structural verdict check decides `pass` or `critical_open`.

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

1. Write the failing tests first:
   - In `crates/horch-core/src/fleetd/task_execution/domain/verification.rs` (`mod tests`, around line 334), add test function `changed_judge_task_with_no_command_skips_command_check_in_plan`:
     - Construct `Facts` for a changed judge task (`base: Some("base")`, `tip: "tip1"`, `check: None`, `check_none: false`, with the effective kind set to `Some("judge")`) and pass it to `verification::plan`.
     - Assert that `verification::plan` returns `Plan::Decided(verdict)` where `verdict.decision == Decision::NotApplicable`, `verdict.reason == JUDGE_NO_CHECK`, and `verdict.verified().is_some()`.
     - Also assert that a non-judge task (`kind: Some("docs")` or `Some("feature")`) with `check: None` and `check_none: false` still returns `Decision::Failed` with `"no check..."`.
   - In `crates/horch-core/tests/judge_pair.rs`, ensure test function `planned_judge_task_with_no_check_passes_on_its_verdict` (and `task_with_no_check_that_is_not_a_judge_is_still_refused`) exists and tests a judge task whose `land_check` is `None` and whose run has no default check, asserting that `w.pass()` records `verification::JUDGE_NO_CHECK` and moves both the judge task and the judged task to `"done"`.
   - Check: run the unit and integration tests through `scripts/dev-sandbox.sh` and see them fail before the fix is in place.
2. In `crates/horch-core/src/fleetd/task_execution/domain/verification.rs` and `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`:
   - Add `pub const JUDGE_NO_CHECK: &str = "a judge task is decided by its structural check and its verdict";` in `verification.rs` (if not already present from T-561).
   - Add the effective task kind (or `kind: Option<&'a str>`) to `verification::Facts<'a>` (or keep `plan_judge` wired into `plan` when `f.kind == Some("judge")`), so `verification::plan` returns `Plan::Decided(Verdict::of(f, Decision::NotApplicable, None, JUDGE_NO_CHECK))` when `f.kind == Some("judge") && f.check.is_none() && f.base != Some(f.tip)`.
   - In `verifier::verify_one` (`crates/horch-core/src/fleetd/task_execution/application/verifier.rs:195`), pass the effective kind of `task` (resolving `"judge"` when `judged_task(&task.kind, task.parent_task_id.as_deref()).is_some()` or from `COALESCE(work_kind, json_extract(spec_json, '$.kind'))`) into `Facts` and `verification::plan`.
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.
- Changing `direct.rs` or `dispatch.rs` (covered by ticket 53 for R5-F6).

## Done when

`changed_judge_task_with_no_command_skips_command_check_in_plan` in `verification.rs` and `planned_judge_task_with_no_check_passes_on_its_verdict` in `crates/horch-core/tests/judge_pair.rs` pass, and the ticket Check passes.
