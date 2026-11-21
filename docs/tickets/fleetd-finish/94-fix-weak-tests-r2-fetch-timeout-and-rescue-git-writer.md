# 94: Strengthen R2 FM-12 and FM-25 tests for fetch timeout, missing origin, and git-writer release for rescue jobs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test fetch_job && scripts/dev-sandbox.sh cargo test -p horch-core --test worktree_jobs && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/fetch_job.rs`, `crates/horch-core/tests/worktree_jobs.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p2-p3.md` (section `## Tests that do not prove their rule`, row 4: `FM-12, FM-25`, watchlist line 72, and `## Operator decisions` item 3) and `docs/specs/fleetd/design.md` (rules FM-12, FM-25, RR-5).

## Goal

Strengthen `crates/horch-core/tests/fetch_job.rs` and `crates/horch-core/tests/worktree_jobs.rs` so they prove that an unrelated `Kind::Fetch` job on a repository with no `origin` remote (or a `Kind::Fetch` process that hangs past its job deadline) terminates or fails within its bound, ingests or abandons cleanly, and releases the shared per-repository git-writer lock (`jobs::git_writer_busy`) so a queued Phase 3 `Kind::Rescue` job on the same repository starts and rescues its orphaned worktree.

## Requirements

- **R2 weak-test point — FM-12, FM-25 (`Phase 3 rescue queue tests`)**:
  - **Existing tests and file:line**:
    - `only_1_git_writer_runs_for_a_repository_at_a_time` at `crates/horch-core/tests/worktree_jobs.rs:744`
    - `failed_fetch_sets_stale_since_and_a_later_pass_clears_it` at `crates/horch-core/tests/fetch_job.rs:372`
    - `fetch_job_process_writes_no_row_and_a_second_ingest_changes_nothing` at `crates/horch-core/tests/fetch_job.rs:475`
  - **Design rules**:
    - **FM-12** (`docs/specs/fleetd/design.md:1557`): "Long work is a job; a pass never waits for it; a heartbeat older than 30s triggers a restart; a job past its time limit is killed by process group, marked `abandoned` and retried with backoff up to 3 times before needs-you."
    - **FM-25** (`docs/specs/fleetd/design.md:1570`): "Orphaned worktree: owner attempt is gone, or folder found on disk with no live owner -> Uncommitted or unpushed work sits in a folder -> fleetd moves the row to `orphaned`, queues a `rescue` job that commits dirty and untracked files to `refs/horch/rescue/<worktree-id>/<n>` without moving the branch, verifies the ref, and moves the row to `rescued`."
  - **Why the existing tests do not prove the rule (from R2)**:
    "They prove queue state transitions, not that an unrelated fetch process releases the shared git-writer queue within the pass or job deadline. Watchlist line 72 remains external evidence of that missing integration proof."
    And R2 Operator decision 3: "Recommendation: require a bounded-process test that proves timeout, result ingestion, and git-writer release when a project has no origin."
  - **Stronger asserts to write**:
    1. In `crates/horch-core/tests/fetch_job.rs`, add test function `fetch_on_project_with_no_origin_fails_promptly_and_releases_git_writer_for_rescue`:
       - Remove the `origin` remote from `w.repo` (`git(&w.repo, &["remote", "remove", "origin"])`).
       - Queue and start a `Kind::Fetch` run on `w.repo()` at `clock = 0`, and queue a `Kind::Rescue` run for an orphaned worktree of the same repository `w.repo()`.
       - Assert that while the `Kind::Fetch` run is in `State::Running`, `jobs::git_writer_busy(&w.conn, w.repo()).unwrap()` is `true`, and `runner.start` for the queued `Kind::Rescue` run returns `Ok(false)` (held by the running git writer).
       - Execute the `fetch` job process (`w.run_job(fetch_id, pid)`) and ingest its result file (`w.ingest()`).
       - Assert that `w.ingest()` returns `vec![(fetch_id, State::Failed)]`, the result text contains `"git fetch origin failed"`, `jobs::git_writer_busy(&w.conn, w.repo()).unwrap()` is now `false`, and `runner.start` for the queued `Kind::Rescue` run now returns `Ok(true)` and moves the `rescue` run to `State::Running`.
    2. In `crates/horch-core/tests/worktree_jobs.rs`, add test function `hung_fetch_process_past_deadline_is_killed_and_releases_git_writer_for_queued_rescue`:
       - Set up `w = world()`, create an orphaned worktree `row = w.made("t1")` with dirty and untracked files on disk, and queue both an unrelated `Kind::Fetch` run (with `time_limit_s = 5`) and the `Kind::Rescue` run for `row.worktree_id` on the same repository `w.repo`.
       - Start the `Kind::Fetch` run with a real sleeping child process (`sleep 60`) via `runner.start` at `clock = 0`.
       - Assert that `jobs::git_writer_busy(&w.conn, &repo).unwrap()` is `true` and attempting to start the queued `Kind::Rescue` run at `clock = 1` returns `Ok(false)`.
       - Advance the fleet clock past the `fetch` deadline (`clock = 6`), run `jobs::observe` in a transaction to observe the overdue `fetch` run, and call `jobs::stop` on each returned `Stop` action.
       - Assert that `jobs::observe` moves the `fetch` run to `State::Abandoned`, `jobs::stop` returns `true` and kills the sleeping `fetch` process group, `jobs::git_writer_busy(&w.conn, &repo).unwrap()` becomes `false`, and the queued `Kind::Rescue` run now starts (`runner.start` returns `Ok(true)`), executes `rescue_git::rescue`, writes the verified `refs/horch/rescue/...` ref, and moves the worktree row to `rescued`.
  - **What to do if the stronger test is red**:
    If a failure is in the test files, fix it inside `**Own:**`. If a product fault is uncovered outside `**Own:**` (for example in `crates/horch-core/src/fleetd/integration/adapters/exec_fetch.rs` or `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`), do not edit outside `**Own:**` — stop and send `BLOCKED:` to the orchestrator with the failing test output and exact `file:line`.

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

1. Inspect `crates/horch-core/tests/fetch_job.rs` (lines 370-515) and `crates/horch-core/tests/worktree_jobs.rs` (lines 740-830).
2. In `crates/horch-core/tests/fetch_job.rs`, add test function `fetch_on_project_with_no_origin_fails_promptly_and_releases_git_writer_for_rescue` as specified in Requirements item 1.
3. In `crates/horch-core/tests/worktree_jobs.rs`, add test function `hung_fetch_process_past_deadline_is_killed_and_releases_git_writer_for_queued_rescue` as specified in Requirements item 2. Ensure `runner.reap()` or process cleanup runs so no child `sleep` process outlives the test (rule 8).
4. Run the ticket Check:
   `scripts/dev-sandbox.sh cargo test -p horch-core --test fetch_job && scripts/dev-sandbox.sh cargo test -p horch-core --test worktree_jobs && cargo fmt --all --check`

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` (owned by ticket `35-fix-r1-f1-complexity-sweep-sandbox.md`).
- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.

## Done when

`fetch_on_project_with_no_origin_fails_promptly_and_releases_git_writer_for_rescue` in `crates/horch-core/tests/fetch_job.rs` and `hung_fetch_process_past_deadline_is_killed_and_releases_git_writer_for_queued_rescue` in `crates/horch-core/tests/worktree_jobs.rs` pass, and the ticket Check passes.
