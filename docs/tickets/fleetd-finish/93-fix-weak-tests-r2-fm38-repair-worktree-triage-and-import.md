# 93: Strengthen R2 FM-38 repair worktree tests and prevent triage from parking unsettled task branches

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test repair_attempts && scripts/dev-sandbox.sh cargo test -p horch-core --test worktree_triage && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/triage/mod.rs`, `crates/horch-core/tests/repair_attempts.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p2-p3.md` (section `## Tests that do not prove their rule`, row 2: `FM-38`, and `## Operator decisions` item 2) and `docs/specs/fleetd/design.md` (rules FM-38, I9, RR-6).

## Goal

Strengthen `crates/horch-core/tests/repair_attempts.rs` to prove that neither normal `triage::collect` nor `import::import_all` can mark or clean the worktree of an unsettled (`conflicted`, `gate_failed`, `ready`, `in_bundle`, or `rebasing`) task branch before that branch is settled (`landed` or `abandoned`), and fix the product fault in `crates/horch-core/src/taskledger/triage/mod.rs` (`is_parked`) where a `done` task whose worker exited had its worktree treated as parked while its `task_branches` row was still in `conflicted` or `gate_failed`.

## Requirements

- **R2 weak-test point — FM-38 (`repair_that_cannot_start_fails_only_its_branch`)**:
  - **Existing test and file:line**:
    - `repair_that_cannot_start_fails_only_its_branch` at `crates/horch-core/tests/repair_attempts.rs:840`.
  - **Design rules**:
    - **FM-38** (`docs/specs/fleetd/design.md:1583`): "A task branch conflicts or fails the gate after its worker exited -> The branch never lands, and its idea never reaches `integrating` -> fleetd owns `conflicted` and `gate_failed`: it starts a resolver attempt or a fix attempt in the worktree; each landing state has a time limit, and past it 1 item goes on the needs-you list."
    - **RR-6** (`docs/specs/fleetd/design.md:159`): "Every task branch shall land through the integration queue; no worker merges to the feature branch directly."
  - **Why the existing test does not prove the rule (from R2)**:
    "It proves isolation and visibility when a worktree row is marked cleaned. It does not prove whether normal triage or import can create that state before a conflicted or gate-failed branch is settled."
  - **Product fault exposed by the stronger test**:
    In `crates/horch-core/src/taskledger/triage/mod.rs:176-216`, `is_parked` checks `!task_pending(row) && !held_by_live_worker(conn, row)?`, where `task_pending` checks only `tasks.status IN ('pending', 'ready', 'escalated', 'running', 'verifying')`. When a task reaches `TaskStatus::Done` (`tasks.status = 'done'`) and its worker has exited, `task_pending(row)` is `false` and `held_by_live_worker` is `false` even while the task's branch in `task_branches` is in `conflicted`, `gate_failed`, `ready`, `in_bundle`, or `rebasing` (not yet `landed` or `abandoned`). As a result, `triage::collect` currently lists the worktree of a `conflicted` or `gate_failed` branch as parked, allowing normal triage to archive or delete the worktree before the branch is settled.
  - **Stronger asserts to write**:
    1. In `crates/horch-core/tests/repair_attempts.rs`, add test function `triage_does_not_list_worktree_of_unsettled_conflicted_or_gate_failed_branch`:
       - Create a task that finishes its first attempt (`w.first_attempt_done()`, so `tasks.status = 'done'`, `worktrees.status = 'open'`, and the first worker is ended) and move its branch in `task_branches` through `"ready"`, `"conflicted"`, and `"gate_failed"` (before `repair::start` binds a repair worker).
       - Call `triage::collect(w.store.conn(), &Git::new("git"), &w.repo, triage::Measure::Ids)`.
       - Assert that `triage::collect` returns empty (`rows.is_empty()`) for every unsettled `landing_state` (`"ready"`, `"in_bundle"`, `"rebasing"`, `"conflicted"`, `"gate_failed"`), so normal triage cannot clean, archive, or delete the worktree before the branch is settled, and a subsequent `w.start(10)` binds the resolver or fix attempt in that worktree.
       - Also assert that once the branch's `landing_state` in `task_branches` becomes `"abandoned"` (and the task has no live worker and the folder is still on disk), `triage::collect` lists the worktree as parked so it can be triaged.
    2. In `crates/horch-core/tests/repair_attempts.rs`, add test function `import_does_not_clean_live_worktree_of_conflicted_or_gate_failed_branch`:
       - Create a task whose branch is `"conflicted"` (or `"gate_failed"`) with its worktree at `w.tree`.
       - Stage and run `import::import_all` with a legacy `worktree_claim` record pointing to the same `path` (`w.tree`) with `"state": "landed"` (the only legacy state that `claim_status` maps to `"cleaned"` when not on disk).
       - Assert that the existing worktree row's `status` remains `"open"` and its `state` remains `"created"`/`"active"` (preserved by `ON CONFLICT(path) WHERE state <> 'removed' DO NOTHING`), and `w.start(10)` still binds and starts the repair attempt in `w.tree`.
  - **What to do if the stronger test is red**:
    Fix the product fault in `crates/horch-core/src/taskledger/triage/mod.rs` (inside `**Own:**`). If any fault is found outside `**Own:**`, do not edit outside `**Own:**` — stop and send `BLOCKED:` to the orchestrator with the evidence.

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

1. Write the 2 new tests in `crates/horch-core/tests/repair_attempts.rs` (after `repair_that_cannot_start_fails_only_its_branch` at line 840):
   - `triage_does_not_list_worktree_of_unsettled_conflicted_or_gate_failed_branch`
   - `import_does_not_clean_live_worktree_of_conflicted_or_gate_failed_branch`
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test repair_attempts -- triage_does_not_list_worktree_of_unsettled_conflicted_or_gate_failed_branch` and see it fail because `triage::collect` currently lists the worktree of a `done` task even when its `task_branches` row is `conflicted` or `gate_failed`.
2. In `crates/horch-core/src/taskledger/triage/mod.rs`:
   - Add helper `branch_unsettled(conn: &Connection, row: &LedgerRow) -> Result<bool>` that queries `task_branches` to check whether an unsettled branch row (`landing_state NOT IN ('landed', 'abandoned')`) exists for `row.task_id` (when `Some`) or for `row.branch` (when `!has_no_branch(&row.branch)`).
   - In `is_parked` (`crates/horch-core/src/taskledger/triage/mod.rs:205`), return `Ok(false)` when `branch_unsettled(conn, row)?` is `true` (unless `nothing_left(git, root, row)` already returned `Ok(false)`).
3. Run the ticket Check:
   `scripts/dev-sandbox.sh cargo test -p horch-core --test repair_attempts && scripts/dev-sandbox.sh cargo test -p horch-core --test worktree_triage && cargo fmt --all --check`

## Out of scope

- Modifying `crates/horch-core/src/taskledger/triage/apply.rs` (owned by ticket `39-fix-r2-f1-triage-apply-per-decision-savepoint.md`).
- Modifying `crates/horch-core/src/fleetd/task_execution/application/repair.rs`.

## Done when

`triage_does_not_list_worktree_of_unsettled_conflicted_or_gate_failed_branch` and `import_does_not_clean_live_worktree_of_conflicted_or_gate_failed_branch` in `crates/horch-core/tests/repair_attempts.rs` pass, `triage::collect` never lists a worktree whose `task_branches` row is unsettled, and the ticket Check passes.
