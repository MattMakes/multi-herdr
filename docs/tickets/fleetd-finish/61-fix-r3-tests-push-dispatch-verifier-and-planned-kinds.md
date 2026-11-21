# 61: Fix R3 weak tests: verify the end-to-end push-dispatch verifier chain (T-601) and non-legacy planned task kinds in task.yaml (FR-4, RR-3, FM-9)

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test task_files && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p4_verifier && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/task_files.rs`, `crates/horch-e2e/tests/p4_verifier.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p4.md` (section "Tests that do not prove their rule", items 3 and 4; watchlist line 48 / T-601; finding R3-F3) and `docs/specs/fleetd/design.md` (rules FR-4, FR-5, RR-3, FM-9).

## Read first: dependency on ticket 53 (R5-F6 / R3-F3)

Ticket 53 (`53-fix-r5-f6-direct-judge-output-ticket-and-kind.md`) updates `dispatch::facts` at `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:583` to read `SELECT COALESCE(work_kind, json_extract(spec_json, '$.kind')) FROM tasks WHERE task_id = ?1` so planned non-legacy kinds (`judge`, `research`, `design`, `creative`) survive dispatch into `task.yaml`. This ticket is blocked by ticket 53 (and thus by T-8 / `08-c5-land-t561.md`) and owns `crates/horch-core/tests/task_files.rs` and `crates/horch-e2e/tests/p4_verifier.rs` (disjoint from ticket 53's `Own` paths).

Note on the other 3 bullets of `## Tests that do not prove their rule` in `end-review-p4.md`:
- Bullet 1 (SR-4 complexity-sweep outside-path test) is covered by ticket 35 (`35-fix-r1-f1-complexity-sweep-sandbox.md`, task T-37) in `crates/horch-core/tests/check_sandbox.rs`.
- Bullet 2 (FM-6 Antigravity Stop hook tests) is covered by T-16 (commits `b1382d1a`, `df3f9299` on `fleet/fleetd-finish`) in `crates/horch-core/tests/harness_defaults.rs:873-944` and by ticket 60 (`60-fix-r3-f2-antigravity-exec-stop-hook.md`) in `crates/horch-core/tests/p7_antigravity_exec.rs`.
- Bullet 5 (SR-3 external messaging tests) is covered by ticket 17 (`17-d2-sr3-external-message.md`) in `crates/horch-core/tests/sr3_no_agent_push.rs` and `crates/horch-e2e/tests/p4_safety.rs`.

## Goal

1. In `crates/horch-e2e/tests/p4_verifier.rs`, prove the full push-dispatch-to-verifier chain requested by T-601 (watchlist line 48): `horch fleetd` dispatches a ready task by push (without `testing::bind_live` or a pre-spawned stand-in `/bin/sh` wrapper), creates its worktree via `worktree_add`, writes `task.yaml` and `inbox/START.md`, launches the fake agent through the launcher and wrapper, ingests `outbox/result.json`, runs the `task_check` job in the worker sandbox, writes `verdict-1.json`, and moves the task to `done`.
2. In `crates/horch-core/tests/task_files.rs`, prove that every non-legacy planned task kind (`judge`, `research`, `design`, `creative`) committed through `planned_tasks::add_planned_tasks` (where `tasks.work_kind` is `NULL` and the kind lives in `tasks.spec_json`) survives `dispatch::write_task_files` into `task.yaml` with `kind: <planned_kind>` rather than `kind: null`.

## Requirements

- **R3 weak test 3 / Watchlist line 48 (T-601 — FR-5, RR-3, FM-9)**:
  - **Review text and file:line**:
    `crates/horch-e2e/tests/p4_verifier.rs:101` uses a stand-in process and manual attempt binding (`testing::bind_live` at line 189 and manual `write_attempt_files` at line 233). It proves result ingestion and verification after binding, but not the real push dispatch, launcher, task-file, and hook chain requested by T-601.
- **R3 weak test 4 / R3-F3 (FR-4, RR-12)**:
  - **Review text and file:line**:
    `crates/horch-core/tests/task_files.rs:17` and `crates/horch-core/tests/task_files_p6.rs:19` manually construct `TaskFacts` with `kind: Some("feature")`. They do not prove that a planned non-legacy kind (`judge`, `research`, `design`, `creative`) survives dispatch from the `tasks` table into `task.yaml`.

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

1. In `crates/horch-core/tests/task_files.rs`, add test function `planned_non_legacy_kinds_survive_dispatch_into_task_yaml`:
   - Open an in-memory or temporary SQLite ledger migrated to `schema::VERSION`, create a run and an idea row, and commit 4 `PlannedTask` entries via `planned_tasks::add_planned_tasks` with `work_kind: None` and `spec_json` setting `"kind"` to `"judge"`, `"research"`, `"design"`, and `"creative"` respectively.
   - Bind each task with `dispatch::bind` (or `testing::bind_live` + `dispatch::write_task_files`) and call `dispatch::write_task_files(conn, fleet_root, &attempt_id, &[])`.
   - Read each generated `task.yaml` and assert that `y["kind"]` equals `"judge"`, `"research"`, `"design"`, and `"creative"` respectively (never `null`).
2. In `crates/horch-e2e/tests/p4_verifier.rs`, add test function `push_dispatched_task_launches_writes_task_files_and_reaches_done_through_verifier`:
   - Set up a `Harness` with `fleet_workspace()` (`w1`), `[gates]\nfast = "true"\nfull = "true"\n` in `fleet.toml`, `HORCH_QUOTA_FILE` pointing to `quota/all-ok.json`, and `HORCH_FAKE_SCENARIO="exec,commit,result"` (as in `crates/horch-e2e/tests/p0_dispatch.rs:88-167`), queuing a `Ready` task with `land_check = Some("test -s fake-claude.txt")` and `VerifyProfile::Minimal.steps()` — without calling `testing::bind_live` or `write_attempt_files` in the test setup.
   - Start `horch fleetd` (`Fleetds::start` or `h.serve_fleetd()`) and wait for `fleetd` itself to bind the task to attempt `A1`, run `worktree_add`, write `task.yaml` and `inbox/START.md`, launch the fake agent through the launcher/wrapper so it commits `fake-claude.txt` and writes `outbox/result.json`, run the `task_check` job in the worker sandbox, write `verdict-1.json` with `"decision": "passed"`, and transition the task to `"done"`.
   - Assert that `task.yaml` and `inbox/START.md` exist in the attempt folder, `verdict-1.json` has `"decision": "passed"`, and the task's transition event to `done` records `"the verifier: verdict-1.json judged"`.
3. Build workspace binaries (`scripts/dev-sandbox.sh cargo build --workspace --bins`) and run the ticket Check.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.
- Modifying `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs` (owned by ticket 53).

## Done when

`planned_non_legacy_kinds_survive_dispatch_into_task_yaml` in `crates/horch-core/tests/task_files.rs` and `push_dispatched_task_launches_writes_task_files_and_reaches_done_through_verifier` in `crates/horch-e2e/tests/p4_verifier.rs` pass, and the ticket Check passes.


## Added 2026-11-20: more red tests of the local baseline in your Own files

These tests are red in `docs/specs/fleetd/gates/baseline-2026-11.md` and are part of this ticket. The ticket is done only when each test of each Own test file passes. Rule: a test of a behavior that Phase 6 retired by design gets its assert moved to the designed new effect; delete it only when no form of the rule exists, and name the test, the retired behavior and its rule in the commit message. No permanent expected-failures entry.

- `fm9_code_task_with_a_failing_check_stays_not_done` — `timed out after 20s waiting for a verifier rejection of T-1`
- `fm9_docs_task_without_its_section_stays_not_done` — `timed out after 20s waiting for a verifier rejection of T-1`
- `passing_code_task_reaches_done_only_through_the_verifier` — `timed out after 20s waiting for T-1 to pass the verifier`
- `sr4_check_that_writes_outside_its_paths_is_refused` — `timed out after 20s waiting for a verifier rejection of T-1`

## Added 2026-11-20: SR-3 e2e test of the verifier check (from task T-17)

Task T-17 gave the check sandbox the rule `Network::Local` (loopback and Unix sockets only) and proved it with a core test. Add the e2e proof here, in `crates/horch-e2e/tests/p4_verifier.rs`, next to `sr4_check_that_writes_outside_its_paths_is_refused`, with the `World` fixture: a task whose check runs `curl` to an off-host address (use a listener on the non-loopback address of the host, as the core test of T-17 does; no real internet host) is rejected by the verifier, and the reason names SR-3 and the network rule. Name: `sr3_check_that_connects_off_host_is_refused`.
