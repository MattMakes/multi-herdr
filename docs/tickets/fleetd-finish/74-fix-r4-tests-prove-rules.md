# 74: Fix R4 test gaps: prove inherited live HORCH_LEDGER_DB isolation and Codex operator-boundary probe (RR-5, SR-8, FM-32)

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib execution::store && scripts/dev-sandbox.sh cargo test -p horch-core --test p7_operator_boundary && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p5_operator_boundary && cargo fmt --all --check`
**Own:** `crates/horch-core/src/execution/store.rs`, `crates/horch-core/tests/p7_operator_boundary.rs`, `crates/horch-e2e/src/harness.rs`, `crates/horch-e2e/tests/p5_operator_boundary.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p5.md` (section "Tests that do not prove their rule", rows 6 and 7) and `docs/specs/fleetd/design.md` (rules RR-5, SR-8, FM-32).

## Goal

Add the missing tests from the Phase 5 end review section "Tests that do not prove their rule" that are not already covered by tickets 70–73 or T-5 (`05-c2-t899-wrapper-operator.md`):
1. Prove that `Harness::seal` and the test harnesses strip or isolate an inherited live `HORCH_LEDGER_DB` and `FLEET_ROOT` from the parent shell environment so a child process never touches an inherited live ledger (RR-5).
2. Prove that `conformance::run_check` / `p7_operator_boundary` exercises the Codex adapter's version and `network_proxy` feature probe together with the sandboxed operator-boundary execution probe, refusing when the Codex version or `network_proxy` probe fails after an update (SR-8, FM-32).

## Requirements

- **R4 Test Gap 1 (RR-5 — `trf_45_session_db_is_the_task_ledger_unless_the_state_dir_moved`)**:
  - **Review text**: "`trf_45_session_db_is_the_task_ledger_unless_the_state_dir_moved`: It uses a synthetic environment and does not prove that every test harness strips an inherited live `HORCH_LEDGER_DB`."
  - **Design rule RR-5** (`docs/specs/fleetd/design.md:158`): "From Phase 5, exactly one process shall write the ledger. Every other component writes files that it ingests."
- **R4 Test Gap 2 (SR-8 / FM-32 — Codex adapter version and rendering tests)**:
  - **Review text**: "Codex adapter version and rendering tests: They prove rendered configuration and a feature probe, but only a real CLI sandbox probe can prove enforcement after a Codex update."
  - **Design rule SR-8** (`docs/specs/fleetd/design.md:182`, `:910`, `:1404`): The conformance check "Operator boundary" proves for each adapter that from inside the sandbox, reading `operator.token`, connecting to the command channel, and writing into `run/requests/` are each refused, and profile validation refuses a profile when the CLI version or sandbox support does not hold.

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

1. Write the test for inherited live `HORCH_LEDGER_DB` isolation in `crates/horch-e2e/tests/p5_operator_boundary.rs` (and `crates/horch-core/src/execution/store.rs`):
   - Add test function `sealed_harness_strips_inherited_live_horch_ledger_db_and_fleet_root`:
     - Create an outer sentinel directory with a canary ledger path `canary_ledger.sqlite3` and canary `fleet_root`.
     - Verify that a command sealed by `Harness::seal` (`crates/horch-e2e/src/harness.rs:345`) does not inherit `HORCH_LEDGER_DB` or `FLEET_ROOT` from the parent process environment, resolves `RuntimeContext::from_env` to the harness's isolated temporary ledger `<harness home>/.horch/ledger.sqlite3`, and never opens or creates the outer canary ledger file.
     - If `Harness::seal` or any test helper in `crates/horch-e2e/src/harness.rs` fails to strip or override `HORCH_LEDGER_DB` / `FLEET_ROOT` when `env_clear()` is not used on a helper path, fix it so `HORCH_LEDGER_DB` and `FLEET_ROOT` are never leaked from the parent shell.
2. Write the test for the Codex operator-boundary conformance probe across version/feature changes in `crates/horch-core/tests/p7_operator_boundary.rs`:
   - Add test function `codex_operator_boundary_conformance_refuses_unproven_version_and_probes_sandbox_acts`:
     - Using `Machine` in `crates/horch-core/tests/p7_operator_boundary.rs`, test `conformance::run_check` for `HarnessKind::Codex` when the Codex binary reports:
       1. A version below `OPERATOR_BOUNDARY_SINCE` (`0.166.0`), asserting the conformance check refuses the profile as unsupported / unhealthy.
       2. Version `0.167.0` without `network_proxy` (`Codex::WithoutNetworkProxy`), asserting it is refused.
       3. Version `0.167.0` with `network_proxy` (`Codex::WithNetworkProxy`), asserting the live sandbox probe executes all 3 operator acts (`ReadToken`, `ConnectChannel`, `WriteRequest`) inside the platform sandbox and passes only when all 3 acts are denied.
3. Note: `crates/horch-core/src/execution/store.rs` and `crates/horch-e2e/tests/p5_operator_boundary.rs` are also touched by T-5 (`05-c2-t899-wrapper-operator.md`) and ticket 70 (`70-fix-r4-f1-queued-request-refusal-visibility.md`), and `crates/horch-core/tests/p7_operator_boundary.rs` is touched by T-4 (`04-c1-land-t898.md`). Mark this ticket `Blocked by: T-4, T-5, 70`.
4. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Running a paid or network-connected production Claude or Codex CLI in automated cargo tests (covered by manual/release gate OD-3).
- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.

## Done when

`sealed_harness_strips_inherited_live_horch_ledger_db_and_fleet_root` and `codex_operator_boundary_conformance_refuses_unproven_version_and_probes_sandbox_acts` pass, and the ticket Check passes.


## Added 2026-11-20: more red tests of the local baseline in your Own files

These tests are red in `docs/specs/fleetd/gates/baseline-2026-11.md` and are part of this ticket. The ticket is done only when each test of each Own test file passes. Rule: a test of a behavior that Phase 6 retired by design gets its assert moved to the designed new effect; delete it only when no form of the rule exists, and name the test, the retired behavior and its rule in the commit message. No permanent expected-failures entry.

- `operator_request_from_a_test_in_a_gate_is_refused_for_each_act` — `timed out after 10s waiting for the task to run`
