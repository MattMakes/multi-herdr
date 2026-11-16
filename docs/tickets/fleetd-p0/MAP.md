# Map: fleetd Phase 0 - reproduce every failure in tests

Spec: `docs/specs/fleetd/design.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [The list of expected failures and the test rule that reads it](01-expected-failures-list.md) |  | 1 |
| 02 | [Red tests for a worker that never claims and a worker that exits without done](02-red-dispatch-and-result.md) | 01 | 2 |
| 03 | [Red tests for an idle claim, a dead agent, a usage limit and a machine sleep](03-red-worker-liveness.md) | 01 | 2 |
| 04 | [Red tests for a killed watchdog, a restart on a new binary and a hung fleetd](04-red-fleetd-supervision.md) | 01 | 2 |
| 05 | [Red tests for a herdr server restart and a worker killed with dirty files](05-red-rescue-and-herdr-restart.md) | 01 | 2 |
| 06 | [Red tests for a forgotten task and 2 ideas sent back to back](06-red-ideas-and-plans.md) | 01 | 2 |
| 07 | [Red tests for several ready task branches and 2 features that conflict](07-red-landing-and-features.md) | 01 | 2 |
| 08 | [The Windows test job of CI](08-windows-test-job.md) |  | 1 |
| 09 | [The gate of Phase 0](09-gate-phase-0.md) | 01, 02, 03, 04, 05, 06, 07, 08 | 3 |

## The check of the run

```
cargo test --workspace --locked
```

This command passes when each test that is not on the list of expected failures passes, and each
test that is on the list fails for its listed reason. The list is the files in
`crates/horch-e2e/expected-failures/` (ticket 01). Each later phase uses this command, and removes
the tests that it makes green from the list.

## Waves

- Wave 1: 01, 08
- Wave 2: 02, 03, 04, 05, 06, 07
- Wave 3: 09

Tickets 01 and 08 run at the same time. Tickets 02 to 07 run at the same time: each owns 1 or 2 test
files and their list files, and none changes a shared file.

## Critical path

01 -> 03 (or 04, 05) -> 09

## Open questions

1. The design does not say how the list of expected failures works. Choice (orchestrator answer to
   Q1): a wrapper `horch_e2e::expected::check` runs a listed test body under `catch_unwind` and
   inverts the result. The list is 1 file for each test file, in `crates/horch-e2e/expected-failures/`,
   so that tickets 02 to 07 own different files. Each entry names the phase that makes it green. The
   test `expected_failures_list_gates_the_run` fails on a stale entry. A listed test whose body passes
   fails the run.
2. The Windows test job fails until Phase P, because many tests of today do not run on Windows.
   Choice (answer to Q2): the job `test-windows` has `continue-on-error: true`. The gate of Phase P
   removes that line.
3. `ddd/08-code/implementation-plan.md`, Slice 0, adds the module folders under
   `crates/horch-core/src/fleetd/`, 3 fitness tests in `crates/horch-core/tests/arch_scan.rs` and rows
   in `docs/architecture.md`. The design's Phase 0 does not name them. Choice (answer to Q3): Phase 0
   leaves them out. Each later phase puts its new code in the module layout of `ddd/08-code`
   (`crates/horch-core/src/fleetd/<context>/`) and adds each fitness test together with the first
   module that it guards.
4. The design does not name the fault point of a hung fleetd pass. Choice: ticket 04 uses
   `HORCH_FAULT=hang-pass`. Phase 1 adds that point to fleetd.
5. The design names commands that do not exist today (`horch fleetd`, `horch fleetd check`,
   `horch idea add`, `horch ideas`). Choice: a red test calls them as the design names them, and its
   listed reason is the missing command. Where a path of today reaches the situation (the watchdog,
   `horch ledger watch --once`, `add-map --dry-run`, `horch ledger finish`), the test uses it, so that
   it fails on the real gap.
6. The tests must not register a launchd job, a systemd unit or a scheduled task (build rule 1). Choice:
   the tests stand in for the service manager and call `horch fleetd check`, as its 30 s timer does.
7. Build rule 7 forbids changes in `docs/specs/fleetd/`, but the gate ticket must write
   `docs/specs/fleetd/gates/fleetd-p0.md`. Choice: ticket 09 owns that 1 file, and states the exception.
8. The e2e test files of today start with `#![cfg(unix)]`, and the harness calls `libc`. Choice: each
   new test file does the same, except `p0_fleetd_hang.rs`, which the design wants on each OS. Its
   Windows result comes from CI after Phase P.

## Coverage

Each mechanism of the row Phase 0 in "What each gate needs", each scenario of the Phase 0 item of the
Migration plan, and each failure mode of the gate line is in exactly 1 ticket. "New work for each
phase" has no row for Phase 0.

| Mechanism, scenario or gate row | Ticket |
| --- | --- |
| The fake harness and the fault points of `crates/horch-e2e` (today) | used by 02 to 07, no new work |
| The list of expected failures and the test rule that reads it | 01 |
| The Windows test job of CI | 08 |
| FM-1: the orchestrator forgets to create a task | 06 |
| FM-2: an agent that never claims | 02 |
| FM-3: one that claims and stops (idle claim caught in under 10 min) | 03 |
| FM-4: an agent process that dies silently | 03 |
| FM-5: a usage-limited worker that must not be reaped | 03 |
| FM-6: one that exits without `done` | 02 |
| FM-7: a killed watchdog | 04 |
| FM-8: two ideas sent back to back | 06 |
| FM-25: a worker killed with dirty and untracked files | 05 |
| A herdr server restart (FM-13, Phase 1 part and full) | 05 |
| Several task branches ready at the same time, and none is forgotten | 07 |
| 2 features that conflict | 07 |
| A machine sleep that must not expire a lease | 03 |
| A daemon restart on a new binary in the middle of a run (FM-34, 04 S10) | 04 |
| A hung fleetd that is restarted, on each OS (FM-12, 06 S14) | 04 |
| The gate: each row above has a red test on the list, and the check of the run passes | 09 |
