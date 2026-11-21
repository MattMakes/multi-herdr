# 22: E4 Phase 6 gate: verify all Phase 6 gate rows and write docs/specs/fleetd/gates/fleetd-p6.md

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/fleetd-p6.md`
**Do not touch:** `crates/`

Spec: `ai_docs/plans/fleetd-finish/plan.md` (section 1 item L11; section 3 unit E4), `ai_docs/plans/fleetd-finish/audit/phase-6.md`, and `docs/tickets/fleetd-p6/65-gate.md`.

## Goal

Run the Phase 6 gate verification inside `scripts/dev-sandbox.sh` and write `docs/specs/fleetd/gates/fleetd-p6.md` as specified by `docs/tickets/fleetd-p6/65-gate.md`. Report any red row as `BLOCKED:` to the orchestrator instead of making a quiet code fix.

## Requirements

- **L11 (Phase 6 gate, `docs/tickets/fleetd-p6/65-gate.md`)**:
  - Verify green rows: FR-1, FR-2, FR-3, FR-4, FR-10, FR-12, RR-3, RR-12, OR-3, FM-1, FM-8, FM-9, FM-10, FM-16, FM-21, FM-29, FM-30, FM-31, FM-32, FM-33, FM-35, FM-39, FM-41, FM-42.
  - Verify the Phase 6 e2e test binaries (`p6_intake`, `p6_sub_orchestrators`, `p6_plans`, `p6_judge_pair`, `p6_questions`, `p6_worker_question`, `p6_fr10_approval`, `p6_features`, `p6_release`, `p6_no_orchestrator_seat`, `p0_ideas`, `p0_landing`), the unit/integration tests of tickets 08 to 12, and REQUIRED items 1 to 5 of `docs/tickets/fleetd-p6/MAP.md`.
  - Verify that no active Phase 6 expected-failure entry remains in `crates/horch-e2e/expected-failures/*.txt` (`grep -n "| 6 |" crates/horch-e2e/expected-failures/*.txt`).
  - A red row is a `BLOCKED:` report to the orchestrator, not a quiet fix.

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

1. Read `docs/tickets/fleetd-p6/65-gate.md`, `docs/tickets/fleetd-p6/MAP.md`, `docs/specs/fleetd/gates/fleetd-p1.md`, and `ai_docs/plans/fleetd-finish/audit/phase-6.md`.
   - Check: list every row required in `docs/specs/fleetd/gates/fleetd-p6.md`.
2. Check for any Phase 6 entry in `crates/horch-e2e/expected-failures/*.txt`:
   `grep -n "| 6 |" crates/horch-e2e/expected-failures/*.txt`
   If any line matches, stop and send `BLOCKED:` to the orchestrator.
   - Check: the grep output is empty.
3. Build the binaries and run each Phase 6 gate test suite and the guard tests through `scripts/dev-sandbox.sh`:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked`
   `scripts/dev-sandbox.sh cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc --test plan_validator --test plan_rules_judges --test judge_pair --test judge_branch --test idea_direct --test question_flow --test question_answers --test p6_authorize`
   `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p6_intake --test p6_sub_orchestrators --test p6_plans --test p6_judge_pair --test p6_questions --test p6_worker_question --test p6_fr10_approval --test p6_features --test p6_release --test p6_no_orchestrator_seat --test p0_ideas --test p0_landing`
   If any test fails, send `BLOCKED:` to the orchestrator with the test name and failure output.
   - Check: all named test binaries pass.
4. Write `docs/specs/fleetd/gates/fleetd-p6.md` recording each requirement, failure mode, gate test, REQUIRED items 1 to 5 of `docs/tickets/fleetd-p6/MAP.md`, the release test, and any pending Linux/Windows CI rows.
5. Run the full ticket Check:
   `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && cargo fmt --all --check`

## Out of scope

- Modifying product or test code in `crates/`.

## Done when

`docs/specs/fleetd/gates/fleetd-p6.md` records every Phase 6 gate row as passing (or pending CI with the allowed reason), no Phase 6 entry exists in `crates/horch-e2e/expected-failures/`, and the Check passes.
