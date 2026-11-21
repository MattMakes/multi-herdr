# 91: Strengthen R1 FM-12 e2e job tests in p1_jobs.rs for ledger_seed, stalled channel client, and bounded daemon stop

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p1_jobs && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p1_jobs.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p0-pp-p1.md` (section `## Tests that do not prove their rule`, row `FM-12`) and `docs/specs/fleetd/design.md` (rules FM-12 and RR-5).

## Goal

Update `crates/horch-e2e/tests/p1_jobs.rs` so its task setup uses `horch_e2e::ledger_seed::RunSeed` instead of the retired `horch ledger run new` CLI command, and add a condition-based e2e test proving that while a long job runs and a client stalls on fleetd's command channel without reading a large reply, `horch fleetd` keeps its pass heartbeat fresh (`!is_hung`), serves subsequent channel clients, and shuts down within a bounded time on `SIGTERM`.

## Requirements

- **R1 weak-test point — FM-12 (`job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted`)**:
  - **Existing test and file:line**:
    - `job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted` at `crates/horch-e2e/tests/p1_jobs.rs:318` (and its helper `checked_task` at `crates/horch-e2e/tests/p1_jobs.rs:195`).
  - **Design rules**:
    - **FM-12** (`docs/specs/fleetd/design.md:1557`): "A pass or a job hangs -> fleetd stops making progress; the watchdog restarts it -> Each pass is 3 short parts (read facts, 1 short ledger transaction, start/stop effects). Long work is a job; a pass never waits for it; a heartbeat older than 30s triggers a restart; a job past its time limit is killed by process group, marked `abandoned` and retried with backoff up to 3 times before needs-you."
    - **RR-5** (`docs/specs/fleetd/design.md:158`): "Only fleetd shall write to the ledger. Every other process (a CLI command, a wrapper, a hook, a job) shall send a request to fleetd or write its own result file."
  - **Why the existing test does not prove the rule (from R1)**:
    "It proves that a long job does not block the pass. It does not cover a blocked channel reply thread or bounded channel shutdown (verified)."
    In addition, `checked_task` at `crates/horch-e2e/tests/p1_jobs.rs:195` still invokes `ok(h, &["ledger", "run", "new", "jobs", "--check", check])` and `ok(h, &["ledger", "add", ...])`, which Phase 6 retired (`horch ledger run new is retired`).
  - **Stronger asserts to write**:
    1. Migrate `checked_task` at `crates/horch-e2e/tests/p1_jobs.rs:195` to seed the run and task with `horch_e2e::ledger_seed::RunSeed::new("jobs").check(check)` and `.task("T1").primary(CODEX_PERSONA)` (matching `p1_pass.rs`), so `p1_jobs.rs` runs on the Phase 6+ ledger.
    2. Add test function `running_job_and_non_reading_channel_client_do_not_block_passes_or_bounded_fleetd_stop` in `crates/horch-e2e/tests/p1_jobs.rs`:
       - Seed a run with several tasks/worktrees (or a large status payload) and start `horch fleetd` with a running `task_check` job (`/bin/sleep 60`).
       - Wait with `wait_for` until the job is in `State::Running` and `<fleet root>/run/fleetd.sock` exists.
       - Connect a raw channel client (`horch_platform::channel::connect(&fleet_root(&h).join("run/fleetd.sock"))`) that sends a valid request line (or holds the stream open without reading the reply).
       - Using `wait_for` (condition-based polling, never a fixed sleep), assert that:
         - `read_heartbeat(&fleet_root(&h))` advances its `pass` counter while the job runs and `!is_hung(&hb, &awake_now().unwrap(), HANG_LIMIT_S)` holds.
         - A subsequent CLI command over the channel (`h.run(&["fleetd", "check"])` and `h.run(&["ledger", "status"])`) succeeds while the first client handle is still open.
         - Sending `SIGTERM` to `horch fleetd` (`stop(fleetd.0, fleetd.1, SIGTERM)`) causes the `horch fleetd` process to terminate cleanly within a bounded time (`wait_for` `!procid::alive(fleetd.0, fleetd.1)` with a 10-second limit).
  - **What to do if the stronger test is red**:
    If the failure is in `crates/horch-e2e/tests/p1_jobs.rs`, fix it inside `**Own:**`. If it reveals a product fault in `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs` (owned by ticket `36-fix-r1-f2-windows-channel-bounded-write.md`) or `crates/horch/src/cmd/fleetd.rs` (owned by ticket `37-fix-r1-f3-pass-loop-event-wake.md`), do not edit outside `**Own:**` — stop and send `BLOCKED:` to the orchestrator with the evidence.

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

1. Inspect `crates/horch-e2e/tests/p1_jobs.rs` (lines 180-460) and `crates/horch-e2e/src/ledger_seed.rs`.
2. In `crates/horch-e2e/tests/p1_jobs.rs`, update `checked_task` (line 195) to seed the run and task via `horch_e2e::ledger_seed::RunSeed` instead of `horch ledger run new` and `horch ledger add`:
   - Seed run `"jobs"` with `.check(check)` and 1 task `"T1"` with `.primary(CODEX_PERSONA)` on `&h`.
   - Bind the worker with `horch_e2e::bind::bind(h, CODEX_PERSONA, Some(agent_pid)).worker_id` and create its worktree with `ok_as(h, &w, &["ledger", "worktree", "new", "work"])`.
   - Also replace the fixed `std::thread::sleep(Duration::from_secs(BACKOFF_S[2] + 4))` in `assert_no_4th_run_starts` (line 446) with a condition check on the fleet clock (`clock_store::read`) advancing past `ended[2].deadline + BACKOFF_S[2] + 2` so the test waits on the deterministic condition rather than an unverified wall-clock sleep.
3. In `crates/horch-e2e/tests/p1_jobs.rs`, add test function `running_job_and_non_reading_channel_client_do_not_block_passes_or_bounded_fleetd_stop`:
   - Start `horch fleetd` with `write_short_job_config(&h)` and a queued `task_check` job (`/bin/sleep 60`).
   - Wait with `wait_for` for the job to reach `State::Running` and record the initial heartbeat `hb0 = read_heartbeat(&fleet_root(&h)).unwrap().unwrap()`.
   - Open a client stream to `fleet_root(&h).join("run/fleetd.sock")` via `horch_platform::channel::connect`, write a request or hold the stream open without reading, and keep the stream handle alive.
   - Wait with `wait_for` (timeout 15 s) for `read_heartbeat(&fleet_root(&h))` to report `hb.pass > hb0.pass` and `!is_hung(&hb, &awake_now().unwrap(), HANG_LIMIT_S)`.
   - Run `ok(&h, &["fleetd", "check"])` and assert that `fleetds.current()` still names the same `(pid, start_time)`.
   - Send `stop(fleetd.0, fleetd.1, SIGTERM)` and wait with `wait_for` (timeout 10 s) for `!procid::alive(fleetd.0, fleetd.1)`.
4. Build the workspace binaries and run the ticket Check:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p1_jobs && cargo fmt --all --check`
   If any test fails due to a product fault outside `**Own:**`, stop and send `BLOCKED:` with the evidence.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs` (owned by ticket `36-fix-r1-f2-windows-channel-bounded-write.md`).
- Modifying `crates/horch/src/cmd/fleetd.rs` (owned by ticket `37-fix-r1-f3-pass-loop-event-wake.md`).

## Done when

All tests in `crates/horch-e2e/tests/p1_jobs.rs` (including `job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted`, `job_past_its_limit_runs_again_after_its_backoff_and_after_3_abandoned_runs_goes_to_you`, and `running_job_and_non_reading_channel_client_do_not_block_passes_or_bounded_fleetd_stop`) pass in the sandbox, and the ticket Check passes.
