# Pre-gate of Phase 4: run the Phase 4 gate rows now, on the tip of 2026-11-21, and report each red row

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/pre-gate-p4.md && grep -q '^## Red rows' docs/specs/fleetd/gates/pre-gate-p4.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/pre-gate-p4.md`
**Do not touch:** `crates/`

Spec: `ai_docs/plans/fleetd-finish/plan.md` (section 1 item L11; section 3 unit E2), `ai_docs/plans/fleetd-finish/audit/phase-4.md`, and `docs/tickets/fleetd-p4/60-gate.md`.

## Goal

Run the Phase 4 gate verification inside `scripts/dev-sandbox.sh` and write `docs/specs/fleetd/gates/fleetd-p4.md` as specified by `docs/tickets/fleetd-p4/60-gate.md`. Report any red row as `BLOCKED:` to the orchestrator instead of making a quiet code fix.

## Requirements

- **L11 (Phase 4 gate, `docs/tickets/fleetd-p4/60-gate.md`)**:
  - Verify green rows: FR-5, SR-2, SR-3, SR-4, PL-4, PL-5, FM-2, FM-6, FM-36, FM-37, FM-38, FM-40, FM-43.
  - Verify part test: FM-9 for a code kind and for the structural check (`p4_verifier`).
  - Verify 1 test each: I8, I9, I10, I11, I12 (`p4_no_loss`).
  - Verify named gate tests (`p4_landing`, `p4_repair`, `p4_remote`, `p4_rescue_refs`, `p4_clock_and_heartbeat`, `p4_safety`, `p0_dispatch`, `p0_landing`) and the Phase 4 release test (`p4_release`).
  - Verify that no active Phase 4 expected-failure entry remains in `crates/horch-e2e/expected-failures/*.txt` (`grep -n "| 4 |" crates/horch-e2e/expected-failures/*.txt`).
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

1. Read `docs/tickets/fleetd-p4/60-gate.md`, `docs/tickets/fleetd-p4/MAP.md`, `docs/specs/fleetd/gates/fleetd-p3.md`, and `ai_docs/plans/fleetd-finish/audit/phase-4.md`.
   - Check: list every row required in `docs/specs/fleetd/gates/fleetd-p4.md`.
2. Check for any Phase 4 entry in `crates/horch-e2e/expected-failures/*.txt`:
   `grep -n "| 4 |" crates/horch-e2e/expected-failures/*.txt`
   If any line matches, stop and send `BLOCKED:` to the orchestrator.
   - Check: the grep output is empty.
3. Build the binaries and run each Phase 4 gate test suite and the guard tests through `scripts/dev-sandbox.sh`:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked`
   `scripts/dev-sandbox.sh cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc --test harness_defaults --test sr3_no_agent_push`
   `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p0_dispatch --test p0_landing --test p4_verifier --test p4_no_loss --test p4_landing --test p4_repair --test p4_remote --test p4_rescue_refs --test p4_clock_and_heartbeat --test p4_safety --test p4_release`
   If any test fails, send `BLOCKED:` to the orchestrator with the test name and failure output.
   - Check: all named test binaries pass.
4. Write `docs/specs/fleetd/gates/fleetd-p4.md` in the format of `docs/specs/fleetd/gates/fleetd-p3.md`, recording each requirement, failure mode, invariant, named test, release test, REQUIRED items 1 to 5 of `docs/tickets/fleetd-p4/MAP.md`, and any pending Linux/Windows CI rows.
5. Run the full ticket Check:
   `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && cargo fmt --all --check`

## Out of scope

- Modifying product or test code in `crates/`.

## Done when

`docs/specs/fleetd/gates/fleetd-p4.md` records every Phase 4 gate row as passing (or pending CI with the allowed reason), no Phase 4 entry exists in `crates/horch-e2e/expected-failures/`, and the Check passes.


## Added 2026-11-21: this is the pre-gate, not the gate

The real gate of Phase 4 runs after the merge of `origin/main` (task E1). This task runs the same rows NOW, so that each red row gets its fix task while other fix tasks still run. Differences to the text above:

- Write `docs/specs/fleetd/gates/pre-gate-p4.md`, not `docs/specs/fleetd/gates/fleetd-p4.md`. First line: the commit of your tip. Then 1 table row for each gate row (rule or test, the test binary and test name that proves it, passed or failed, the first failure line). Then the heading `## Red rows`: 1 item for each red row with the failure text, the file and line, your reading of the cause (label it verified or not verified), and the fix task that owns it if one exists (`horch ledger status --run fleetd-finish`; the tickets are in `docs/tickets/fleetd-finish/`). Write `none` when no row is red. Then `## Rules with no test`: each rule of the gate for which you find no test that proves it.
- A red row does NOT stop the task and is NOT a `BLOCKED:` report: record it and go on with the next row. An expected-failures entry of the phase is a red row: record its owner.
- Do NOT run the full workspace test command of the old Check (step 5). Run only the named suites of the gate, 1 `cargo test` command at a time, with `--no-fail-fast`, and with `--test-threads=4` when the 1-minute load is above 32.
- A test that fails: run it 2 more times alone. Record 3 of 3 red, or flaky with the counts. A flaky test is a red row.
- Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet. Write 1 `horch ledger progress` line each 10 minutes.
