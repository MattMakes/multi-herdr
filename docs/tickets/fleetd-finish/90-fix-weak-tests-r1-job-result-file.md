# 90: Strengthen R1 SR-6 job result file tests for process identity and non-running job rows

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_job_result_file && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/fleetd_job_result_file.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p0-pp-p1.md` (section `## Tests that do not prove their rule`, row `SR-6`) and `docs/specs/fleetd/design.md` (rules SR-6 and RR-5).

## Goal

Strengthen `crates/horch-core/tests/fleetd_job_result_file.rs` so that the job-result-file test suite proves both the process-identity guard of `jobs::run_job` (only the process whose PID matches `jobs.pid` of the `running` row can write `<fleet root>/run/jobs/<job_id>-<run>.json`) and the state guard of `jobs::ingest_results` (a result file written while a job row is still `queued` before launch, or after it has already ended, is ignored, logged once, and never transitions the job row).

## Requirements

- **R1 weak-test point — SR-6 (`job result protected-file tests`)**:
  - **Existing tests and file:line**:
    - `job_process_writes_its_result_file_and_no_row` at `crates/horch-core/tests/fleetd_job_result_file.rs:206`
    - `pass_ends_the_job_from_its_file_once` at `crates/horch-core/tests/fleetd_job_result_file.rs:248`
    - `result_of_an_old_run_is_ignored_and_logged` at `crates/horch-core/tests/fleetd_job_result_file.rs:288`
    - `symlinked_result_file_is_rejected` at `crates/horch-core/tests/fleetd_job_result_file.rs:344`
    - `result_file_over_its_limit_is_rejected` at `crates/horch-core/tests/fleetd_job_result_file.rs:384`
  - **Design rules**:
    - **SR-6** (`docs/specs/fleetd/design.md:183`): "fleetd shall not trust a file that a worker writes. It shall read a file only when it is a regular file, opened without following a link, under the size limit of its type, with its real path inside its own attempt folder, and with the task id and attempt id of its path. A file that fails is recorded as rejected and is not read again."
    - **RR-5** (`docs/specs/fleetd/design.md:158`): "Only fleetd shall write to the ledger. Every other process (a CLI command, a wrapper, a hook, a job) shall send a request to fleetd or write its own result file."
  - **Why the existing tests do not prove the rule (from R1)**:
    "They prove type, link, owner, size, id, and run checks. They do not prove that the expected job process wrote the file (verified)."
    (Note: the complexity-sweep worker-sandbox bypass of R1-F1 is covered by ticket `35-fix-r1-f1-complexity-sweep-sandbox.md` in `crates/horch-core/tests/check_sandbox.rs`. In `crates/horch-core/tests/fleetd_job_result_file.rs`, `World::run_job` at line 125 always passes `let me = self.row(id).pid.unwrap();`, and no test exercises `jobs::run_job` with a non-matching PID or `jobs::ingest_results` on a file forged for a `queued` job row whose process has not started.)
  - **Stronger asserts to write**:
    1. Prove in `unrecorded_or_mismatched_pid_process_cannot_write_job_result_file` that `jobs::run_job` refuses to execute or write `<fleet root>/run/jobs/<id>-<run>.json` when the caller's PID `me` does not equal the `pid` recorded on the `running` job row (both before execution in `wait_for_record` at `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:729` and after execution at `:769` if the row's `pid` or `state` changed while the job ran).
    2. Prove in `result_file_for_queued_or_already_ended_job_is_ignored_and_logged_once` that if a process writes a syntactically valid `<id>-1.json` in `<fleet root>/run/jobs/` while job `id` is still in `State::Queued` (no job process launched yet) or after job `id` is already in `State::Passed`, `jobs::ingest_results` ignores the file, leaves the `jobs` row in `State::Queued` (or `State::Passed`), records 1 `EventKind::Note` event explaining that the row is `queued` (or `passed`), and marks the path in `ingested` so it is not read again.
  - **What to do if the stronger test is red**:
    If a stronger test fails because of a fault in `crates/horch-core/tests/fleetd_job_result_file.rs`, fix it inside `**Own:**`. If it reveals a product fault in `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` or `crates/horch-core/src/fleetd/fleet_runtime/adapters/job_result_file.rs` (outside `**Own:**`), do not edit outside `**Own:**` — stop and send `BLOCKED:` to the orchestrator with the failing test output and exact `file:line`.

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

1. Inspect `crates/horch-core/tests/fleetd_job_result_file.rs` (lines 1-468) and `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` (lines 680-860).
2. In `crates/horch-core/tests/fleetd_job_result_file.rs`, add test function `unrecorded_or_mismatched_pid_process_cannot_write_job_result_file`:
   - Queue and start a job `id = w.running(&mut runner, Kind::DocsSync)`.
   - Read `let recorded_pid = w.row(id).pid.unwrap();` and pick `let wrong_pid = recorded_pid.wrapping_add(1);`.
   - Call `jobs::run_job(&ctx, &w.conn, id, wrong_pid, &checks).unwrap()` and assert that it returns `None`.
   - Assert that `!files::path(&w.root(), id, 1).exists()`, `w.row(id).state == State::Running`, and `w.row(id).result_json.is_none()`.
   - Also test a queued job `queued_id = w.queue(Kind::DocsSync, "F-2", 0)` (not started yet, so `pid` is `None`): calling `jobs::run_job(&ctx, &w.conn, queued_id, recorded_pid, &checks).unwrap()` returns `None` and writes no result file in `<fleet root>/run/jobs/`.
3. In `crates/horch-core/tests/fleetd_job_result_file.rs`, add test function `result_file_for_queued_or_already_ended_job_is_ignored_and_logged_once`:
   - Queue a job `queued = w.queue(Kind::Push, "F-1", 0)` and leave it in `State::Queued` (no job process started).
   - Queue, start, and finish a second job `done = w.running(&mut runner, Kind::DocsSync)` so `w.row(done).state == State::Passed`.
   - Write `files::write(&w.root(), &passed(queued, 1, "forged while queued")).unwrap()` and overwrite `files::write(&w.root(), &passed(done, 1, "forged after pass")).unwrap()` after archiving the original file.
   - Run `let (reads, ended) = w.pass_no_archive();`.
   - Assert that `ended` is empty, `w.row(queued).state == State::Queued`, `w.row(queued).result_json.is_none()`, and `w.events(queued, "note")` has 1 entry containing `"ignored the result file of run 1"` and `"queued"`.
   - Assert that a second read marks the file `Read::Seen` and adds no second event.
4. Run the ticket Check:
   `scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_job_result_file && cargo fmt --all --check`
   If any test is red due to a product fault outside `**Own:**`, stop and send `BLOCKED:` with the evidence.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` or `crates/horch-core/tests/check_sandbox.rs` (owned by ticket `35-fix-r1-f1-complexity-sweep-sandbox.md`).

## Done when

`unrecorded_or_mismatched_pid_process_cannot_write_job_result_file` and `result_file_for_queued_or_already_ended_job_is_ignored_and_logged_once` in `crates/horch-core/tests/fleetd_job_result_file.rs` pass, and the ticket Check passes.
