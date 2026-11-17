# 15: Make the Phase 0 red tests of liveness green: the idle claim, the usage limit and the machine sleep

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --test p0_liveness --test expected_failures && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p0_liveness.rs`, `crates/horch-e2e/expected-failures/p0_liveness.txt`

Spec: `docs/specs/fleetd/design.md`, FM-3, FM-5, RR-11, "Supervision, liveness and reconciliation" ("Fleet clock", "Stall ladder"), "Migration plan" item 4 (*Gate:* and Tests: "a usage-limited worker is not reaped and burns no attempt; the idle-claim scenario is caught in under 10 min").

## Goal

The 3 red tests of Phase 2 in `crates/horch-e2e/tests/p0_liveness.rs` pass, and their lines leave `crates/horch-e2e/expected-failures/p0_liveness.txt`: `fm3_idle_claim_is_caught_in_under_10_min`, `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` and `sleep_does_not_expire_a_lease`.

## Requirements

- **FM-3**: an idle claim is caught in under 10 min (stall 300 s and lease 120 s, on the fleet clock).
- **FM-5** and **RR-11**: a usage-limited worker is not reaped as stuck, and its attempt count does not change. Its task restarts on the fallback of another provider, or waits in `waiting_quota`.
- The fleet clock: a sleep of the machine does not expire a lease.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q5, Q9, Q14 and Q15 apply here).
- Each test runs inside `check` of `crates/horch-e2e/src/expected.rs`, and the list file names the reason of each expected failure (`crates/horch-e2e/expected-failures/README.md`). Remove the 3 lines. Keep the line of each other phase.
- `fm3_idle_claim_is_caught_in_under_10_min` reads the run columns of today (`check_every_s` x `stall_rounds`). Those flags stay accepted and no longer decide a reap (MAP.md Q14, ticket 14), so the body of today tests a rule that is gone. Rewrite the body to test the mechanism of the design: a worker whose agent is a live `sleep` (no heartbeat after its claim, no output) claims its task. The test runs `horch ledger watch --once` (1 pass of fleetd, Phase 1) with `HORCH_NOW` steps, and a `fleet.toml` in the fleet root of the harness with a large `pass_period_s`, so that each pass moves the fleet clock far (a pass adds at most 2 pass periods). Assert that the task leaves `running` (to `stalled`, then `ready` or `escalated`) before the fleet clock passes 600 s after the claim. Keep the test name.
- `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` asserts that the agent stays alive. The design stops it: the outcome `quota` ends the attempt, and the task restarts on a new worker of the fallback or waits in `waiting_quota` with no process (MAP.md, Q15; ticket 11). Change that assertion: the attempt of the agent ended with the outcome `quota`, and its process is gone. Keep the other assertions: the verdict is never `dead`, the attempt count does not change, and the task is on `codex-sol` or in `waiting_quota`. The fixture `claude-exhausted-codex-ok.json` stays.
- `sleep_does_not_expire_a_lease` reads `leases.expires_at` and `fleetd.fleet_clock_s` (ticket 02). It needs no change of body unless a name of the code differs.
- A test in this file may not depend on the order of the tests or on the wall clock of the machine. Each harness has its own temporary folders.

## Steps

1. Change the bodies of `fm3_...` and `fm5_...` as above. Run them against the base of this ticket with the 3 lines still in the list, and see that the 3 tests pass (the code of tickets 08 to 14 is there).
2. Remove the 3 lines from `p0_liveness.txt`. Run the Check and the guard tests. Commit.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes. The 3 tests pass and are off the list of expected failures, and the body of each tests the mechanism of the design.
