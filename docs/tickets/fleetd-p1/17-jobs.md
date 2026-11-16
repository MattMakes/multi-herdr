# 17: The jobs table at work: job kinds, the run number, the bound on abandoned runs, and the job runner

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test fleetd_jobs`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/jobs.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/jobs_store.rs`, `crates/horch-core/tests/fleetd_jobs.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" (Long work is a job. A pass never waits for it; The kinds of job), "Ledger" (table `jobs`), FM-12, "Migration plan" ("The numbers of the design": `abandoned_run_limit`, `job_backoff_s`, `job_limit_s`); `ddd/08-code/fleet-runtime/design.md` (ObserveJobs, RequeueJob, JobQueue).

## Goal

Each piece of work that can take more than 1 s can run as a job: a `jobs` row first, then its own process outside the pass, alive by PID and start time, stopped at its time limit. The pass observes each job. A lost or stopped job ends `abandoned` and runs again after its backoff. After 3 abandoned runs in a row of 1 key, no new run starts, and 1 needs-you item names the job. `requeue` starts it again.

## Requirements

- **FM-12** (part): long work is a job outside the pass, so a long job does not delay the heartbeat.
- The mechanisms of the gate row of Phase 1: "the `jobs` table", "the job kinds, the run number, `abandoned_run_limit`, `job_backoff_s` and `horch job requeue`". The CLI `horch job requeue` and `horch job run` are wired in ticket 23; this ticket gives the use cases.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The table comes from ticket 03. Each kind of the design with its class and input key goes into `domain/jobs.rs` as data (the table "The kinds of job"): pure kinds `task_check`, `fast_gate`, `full_gate`, `trial_merge`; effect kinds the other 10.
- The rules (the design): at most 1 run of 1 kind and subject is `queued` or `running` (the index `jobs_one_active`); a pure result `passed`, or `failed` after a run to its end, is used again for the same input key; an effect result is never used again, and each need is a new run with the next `run`; a run after an `abandoned` run has `not_before` = the fleet clock + the backoff (`job_backoff_s`: 30, 120, 480 s); at `abandoned_run_limit` (3) abandoned runs in a row of 1 kind, subject and input key, no new run and 1 needs-you item with the key `job_abandoned:<kind>:<subject>:<input key>`; a new input key starts a new count.
- Use cases in `application/jobs.rs`: `queue(kind, subject, repo, input_key)` (inside the transaction of part 2; it returns the stored result of a reusable pure run), `due_to_start(clock)`, `observe(facts, clock)` (an ended process gives `passed` or `failed` from its `result_json`; a `running` job whose process is gone gives `abandoned`; a job past its `deadline` is marked to stop, and its stop is an effect), `requeue(job key)` (sets `abandoned_runs` to 0 and queues 1 more run). Each state change goes through `transition()` of ticket 11. The time limit of a kind comes from `job_limit_s` of ticket 04; `task_check` uses `CHECK_TIMEOUT` of `crates/horch-core/src/taskledger/finish/land.rs`.
- The job process (MAP.md, Q9): the runner starts `<the horch binary> job run <job id>` through the launcher of ticket 16, after the commit (an effect of part 3). Give `run_job(ctx, job id)`: it reads the row and runs the executor of its kind, and writes `result_json` and the end state. In Phase 1 only `task_check` has an executor: it runs the check command of the task (the `check` of the task row, as `crates/horch-core/src/taskledger/finish/` reads it) in the worktree of the task, with its time limit. Each other kind ends `failed` with the result "no executor before phase <n>", and Phase 1 queues none of them. Ticket 23 wires `horch job run`.
- The fleet clock comes from ticket 15. Each deadline and `not_before` is a value on it.
- Tests use temp folders and a temp ledger (build rule 1). A job process in a test is a small command (`sleep`, `true`, `false`) through the launcher.

## Steps

1. Write the tests first, in the new file `crates/horch-core/tests/fleetd_jobs.rs` and in `domain/jobs.rs`. Each fails before the code exists:
   - `pure_job_result_is_used_again_and_effect_job_runs_again` (Slice 1).
   - `job_abandoned_3_times_goes_on_the_needs_you_list_and_requeue_resets_it` (Slice 1).
   - `running_job_with_a_lost_process_runs_again_after_restart` (Slice 1): a `running` row whose PID is gone ends `abandoned`, and its next run has `not_before` set.
   - `job_past_its_deadline_is_marked_to_stop_and_ends_abandoned`.
   - `second_queue_of_1_kind_and_subject_while_1_runs_adds_no_row`.
   - `new_input_key_starts_a_new_count`.
   - `each_kind_of_the_design_has_its_class_and_input_key`.
2. Write the code. Run the Check.

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

The Check passes. A lost job runs again after its backoff, and after 3 abandoned runs it waits for `requeue` with 1 needs-you item.
