# 27: The e2e tests of Phase 1 for jobs: a 5 min job keeps fleetd healthy; a job past its limit runs again, and after 3 abandoned runs goes to you

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --test p1_jobs`
**Own:** `crates/horch-e2e/tests/p1_jobs.rs`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 3 (Tests: "a job of the fake harness runs for 5 min while fleetd stays healthy and is not restarted"; "a job stopped at its time limit runs again after its backoff, and after 3 `abandoned` runs it puts 1 item on the needs-you list, and `horch job requeue` starts it again"), "Supervision, liveness and reconciliation" (Long work is a job), FM-12.

## Goal

2 gate tests of Phase 1 at the level of the `horch` binary, with a real fleetd in stand-in mode: a long job does not hang the pass, and the job bounds work end to end.

## Requirements

- The 2 job tests of the gate of Phase 1 ("Migration plan" item 3).
- **FM-12** (part): a pass never waits for a job, so a long job never makes fleetd look hung.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The code is in place (tickets 17, 21, 23). This ticket adds tests only. If a test fails for a fault in the code, send a QUESTION with the fault and a proposed fix task.
- The 5 min job (MAP.md, Q19): a `task_check` job whose check is `sleep 300`. `task_check` uses the check timeout of today, 20 min (`CHECK_TIMEOUT`, `crates/horch-core/src/taskledger/finish/land.rs:31`), so the job ends `passed`. Queue it through the API of ticket 17 (Phase 1 queues no job by itself, MAP.md, Q9) on a temp ledger.
- Short numbers for the second test: write a `fleet.toml` in the temp fleet root (ticket 04) with a small `job_limit_s` for `task_check` (for example 5) and small `job_backoff_s` (for example [1, 2, 4]), so the test ends in less than 1 min. The rule is the same with the starting values.
- Start fleetd as the other e2e tests do (stand-in mode, MAP.md, Q8; copy `start_fleetd` and `Fleetds` from `crates/horch-e2e/tests/p0_supervision.rs`). Read the `fleetd` row and the `jobs` table with `horch_core::taskledger::Store`.
- The whole file must run in less than 20 min. Use 1 fleetd for each test, and let the 2 tests run in parallel.

## Steps

1. Write the tests in the new file `crates/horch-e2e/tests/p1_jobs.rs`:
   - `job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted`: during the job, run `horch fleetd check` every 30 s (it restarts nothing), the PID and the start time in the `fleetd` row do not change, and the heartbeat is never older than 30 s. The job ends `passed`.
   - `job_past_its_limit_runs_again_after_its_backoff_and_after_3_abandoned_runs_goes_to_you`: a `task_check` job with the check `sleep 600` ends `abandoned` 3 times, each next run starts only after its backoff on the fleet clock, then 1 needs-you item is open and no 4th run starts; `horch job requeue <job>` starts run 4.
2. Run the Check. If a test fails, send a QUESTION.

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

The Check passes in less than 20 min. The 2 job tests of the gate pass.
