# Pre-gate of Phase 5: run the Phase 5 gate rows now, on the tip of 2026-11-21, and report each red row

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/pre-gate-p5.md && grep -q '^## Red rows' docs/specs/fleetd/gates/pre-gate-p5.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/pre-gate-p5.md`
**Do not touch:** `crates/`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (section 1 items L11, Q1, Q2; section 3 unit E3), `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/phase-5.md`, and `docs/tickets/fleetd-p5/49-gate.md`.

## Goal

Run the Phase 5 gate verification inside `scripts/dev-sandbox.sh` and write `docs/specs/fleetd/gates/fleetd-p5.md` as specified by `docs/tickets/fleetd-p5/49-gate.md`. Report any red row as `BLOCKED:` to the orchestrator instead of making a quiet code fix.

## Requirements

- **L11 (Phase 5 gate, `docs/tickets/fleetd-p5/49-gate.md`)**:
  - Verify green rows: FR-7, RR-5, RR-8, SR-5, SR-6, SR-8, FM-15, FM-17, FM-23, FM-24.
  - Note open design text questions Q1 (FR-7: 9 fixed lines in `TYPED_LINES` vs 1 line in `design.md:143`) and Q2 (SR-8: OS sandbox refusal leaves no `requests` row vs server refusal after sandbox bypass) as described in `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` section "Design text questions".
  - Verify the architecture test (`arch_scan -- fitness_only_fleet_runtime_opens_the_ledger_writable`), the Phase 5 e2e suites (`p5_operator_boundary`, `p5_untrusted_files`, `p5_requests`, `p5_read_only_worker`, `p5_push_stop`), and the Phase 5 release test (`p5_release`).
  - Verify that no active Phase 5 expected-failure entry remains in `crates/horch-e2e/expected-failures/*.txt` (`grep -n "| 5 |" crates/horch-e2e/expected-failures/*.txt`).
  - A red row is a `BLOCKED:` report to the orchestrator, not a quiet fix.

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

1. Read `docs/tickets/fleetd-p5/49-gate.md`, `docs/tickets/fleetd-p5/MAP.md`, `docs/specs/fleetd/gates/fleetd-p1.md`, and `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/phase-5.md`.
   - Check: list every row required in `docs/specs/fleetd/gates/fleetd-p5.md`.
2. Check for any Phase 5 entry in `crates/horch-e2e/expected-failures/*.txt`:
   `grep -n "| 5 |" crates/horch-e2e/expected-failures/*.txt`
   If any line matches, stop and send `BLOCKED:` to the orchestrator.
   - Check: the grep output is empty.
3. Build the binaries and run each Phase 5 gate test suite and the guard tests through `scripts/dev-sandbox.sh`:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked`
   `scripts/dev-sandbox.sh cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc --test fleetd_single_writer --test fr7_typed_lines --test fleetd_lock --test fleetd_channel --test peer_process`
   `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p5_operator_boundary --test p5_untrusted_files --test p5_requests --test p5_read_only_worker --test p5_push_stop --test p5_release`
   If any test fails, send `BLOCKED:` to the orchestrator with the test name and failure output.
   - Check: all named test binaries pass.
4. Write `docs/specs/fleetd/gates/fleetd-p5.md` recording each requirement, failure mode, REQUIRED map items, arch test, release test, CI-only rows (such as Windows single-instance named pipe), and design text questions Q1 and Q2.
5. Run the full ticket Check:
   `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && cargo fmt --all --check`

## Out of scope

- Modifying product or test code in `crates/` or editing `docs/specs/fleetd/design.md`.

## Done when

`docs/specs/fleetd/gates/fleetd-p5.md` records every Phase 5 gate row as passing (with Q1, Q2, and CI-only rows noted), no Phase 5 entry exists in `crates/horch-e2e/expected-failures/`, and the Check passes.


## Added 2026-11-21: this is the pre-gate, not the gate

The real gate of Phase 5 runs after the merge of `origin/main` (task E1). This task runs the same rows NOW, so that each red row gets its fix task while other fix tasks still run. Differences to the text above:

- Write `docs/specs/fleetd/gates/pre-gate-p5.md`, not `docs/specs/fleetd/gates/fleetd-p5.md`. First line: the commit of your tip. Then 1 table row for each gate row (rule or test, the test binary and test name that proves it, passed or failed, the first failure line). Then the heading `## Red rows`: 1 item for each red row with the failure text, the file and line, your reading of the cause (label it verified or not verified), and the fix task that owns it if one exists (`horch ledger status --run fleetd-finish`; the tickets are in `/Users/matthewscott/repo/multi-herdr/docs/tickets/fleetd-finish/`). Write `none` when no row is red. Then `## Rules with no test`: each rule of the gate for which you find no test that proves it.
- A red row does NOT stop the task and is NOT a `BLOCKED:` report: record it and go on with the next row. An expected-failures entry of the phase is a red row: record its owner.
- Do NOT run the full workspace test command of the old Check (step 5). Run only the named suites of the gate, 1 `cargo test` command at a time, with `--no-fail-fast`, and with `--test-threads=4` when the 1-minute load is above 32.
- A test that fails: run it 2 more times alone. Record 3 of 3 red, or flaky with the counts. A flaky test is a red row.
- Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet. Write 1 `horch ledger progress` line each 10 minutes.
