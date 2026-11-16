# 09: The gate of Phase 0

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test --workspace --locked && test -f docs/specs/fleetd/gates/fleetd-p0.md`
**Own:** `docs/specs/fleetd/gates/fleetd-p0.md`, `crates/horch-e2e/tests/p0_dispatch.rs`, `crates/horch-e2e/tests/p0_liveness.rs`, `crates/horch-e2e/tests/p0_supervision.rs`, `crates/horch-e2e/tests/p0_fleetd_hang.rs`, `crates/horch-e2e/tests/p0_rescue.rs`, `crates/horch-e2e/tests/p0_ideas.rs`, `crates/horch-e2e/tests/p0_landing.rs`, `crates/horch-e2e/expected-failures/`, `crates/horch-e2e/src/expected.rs`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 1 (Phase 0) and the tables "What each gate
needs", "Where each rule becomes green" and "What a release does to a run in flight".

## Goal

The gate of Phase 0 is proven: FM-1 to FM-8 and FM-25 each have a red test, and each new scenario of
Phase 0 has a red test. Each red test is on the list of expected failures with the phase that makes it
green. `cargo test --workspace --locked` passes. The file `docs/specs/fleetd/gates/fleetd-p0.md`
records each row.

## Requirements

No requirement or failure mode becomes green in Phase 0 ("Where each rule becomes green": None).
The gate line: "FM-1 to FM-8 and FM-25 each have a red test, and each new scenario above has a red
test". Phase 0 has no "run in flight" test ("What a release does to a run in flight": None).

## Context

- Tickets 01 to 08 of `docs/tickets/fleetd-p0/MAP.md` made the list, the red tests and the Windows
  job. Read `crates/horch-e2e/expected-failures/README.md`.
- The rows of the gate, with the test and the ticket that wrote it:

| Gate row | Test | Ticket | Green in |
| --- | --- | --- | --- |
| FM-1 | `fm1_plan_with_an_uncovered_requirement_is_refused` | 06 | Phase 6 |
| FM-2, agent that never claims | `fm2_agent_that_never_claims_has_its_task_bound_at_spawn` | 02 | Phase 4 |
| FM-3, claims and stops | `fm3_idle_claim_is_caught_in_under_10_min` | 03 | Phase 2 |
| FM-4 | `fm4_dead_agent_ends_its_attempt_within_30_s` | 03 | Phase 1 |
| FM-5, usage-limited worker not reaped | `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` | 03 | Phase 2 |
| FM-6, exits without done | `fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt` | 02 | Phase 4 |
| FM-7, killed watchdog | `fm7_killed_watchdog_does_not_stop_the_reconcile` | 04 | Phase 1 |
| FM-8, 2 ideas back to back | `fm8_two_ideas_sent_back_to_back_stay_2_ideas` | 06 | Phase 6 |
| FM-25, killed with dirty and untracked files | `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing` | 05 | Phase 3 |
| herdr server restart (FM-13, Phase 1 part) | `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` | 05 | Phase 1 |
| herdr server restart (FM-13, full) | `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it` | 05 | Phase 3 |
| several ready task branches | `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten` | 07 | Phase 4 |
| 2 features that conflict | `two_conflicting_features_keep_their_tips_and_reach_a_decision` | 07 | Phase 6 |
| machine sleep | `sleep_does_not_expire_a_lease` | 03 | Phase 2 |
| daemon restart on a new binary | `fm34_daemon_restart_on_a_new_binary_keeps_the_run` | 04 | Phase 1 |
| hung fleetd, each OS | `fm12_hung_fleetd_is_restarted_on_each_os` | 04 | Phase 1 |
| list of expected failures | `expected_failures_list_gates_the_run` | 01 | Phase 0 (passes) |
| Windows test job | job `test-windows` in `.github/workflows/ci.yml` | 08 | CI only |

## Steps

1. Run `cargo test -p horch-e2e --test expected_failures`. It must pass: no stale entry, each entry
   with a phase.
2. For each row of the table above: check that the test exists, that it is on the list with the
   phase of the table, and that it fails for its listed reason (run it alone and read the panic
   message). A reason that is a fault of the test or of the harness is a small fault: correct it in
   the test or in its list line.
3. Run `cargo test --workspace --locked`. It must pass. If a test of today fails, find out if it
   fails on `fleet/fleetd-build` before Phase 0 also (a flaky test of today): send NOTE with its
   name, and do not put it on the list. The list holds only red tests of Phase 0.
4. Check that `.github/workflows/ci.yml` has the job `test-windows`. This Mac cannot see its result:
   it comes from CI on the next pull request to `main`.
5. Write `docs/specs/fleetd/gates/fleetd-p0.md`: a title, the command of the run
   (`cargo test --workspace --locked`), the commit that you tested, and 1 table row for each gate row
   above with the columns gate row, test name, phase that makes it green, result (`red, listed` or
   `passes`). Add the duration of the full run, and the note that the Windows result comes from CI.
6. Run the Check.

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

- Exception to rule 7 and rule 8 for this ticket: the run gives this ticket the file
  `docs/specs/fleetd/gates/fleetd-p0.md` in `docs/specs/fleetd/`. Change no other file of
  `docs/specs/fleetd/`.
- A large fault (a missing test, a test that needs a new fake) is not a small fault: send QUESTION.

## Done when

- The Check passes.
- Each row of the table has its test, its phase on the list, and its result in
  `docs/specs/fleetd/gates/fleetd-p0.md`.
- The gate file names the commit and the duration of the full run.
