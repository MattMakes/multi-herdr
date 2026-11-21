# CI run 6 of fleetd, 2026-11-21

- Run: 44936097033, https://github.com/MattMakes/multi-herdr/actions/runs/44936097033
- Trigger: `workflow_dispatch` on `fleet/fleetd-finish`, started 2026-11-21 18:32Z, ended 20:00Z. Result: `failure`.
- Head commit of the run: `2906f9cc236198c12280f2115f3ba69d110c453b` (T-404 comment sweep). `git ls-remote origin refs/heads/fleet/fleetd-finish` gave the same commit, and the local branch was the same. The check "headSha equals the tip" holds. 72 commits landed between the head of run 5 (`05ff4081`) and this head.
- Compared with: run 5, `docs/specs/fleetd/gates/ci-run-5.md` (run 44890393381, head `05ff4081`).
- Source of the counts: the lines `test result:` of each job log (`gh api repos/.../actions/jobs/<id>/logs`), the lines `test <name> ... FAILED`, and the blocks `---- <test> stdout ----`. For Windows I also compared the test names of the log of run 5 with this log. I ran no test locally (ticket rule). A cause that the log does not name in the failure line is marked "probable" or "not proved".
- Result: 3 of 6 jobs passed (run 5: 2 of 6). `docs` is green now. `test-windows` ended by itself after 35 min: no hang, no limit.

| Job | OS | Result | Time |
|---|---|---|---|
| `test (macos-15)` | macOS | failure: 5338 passed, 6 failed (5 distinct tests), 26 ignored, 387 binaries | 88 min |
| `test (ubuntu-latest)` | Linux | failure: 5340 passed, 5 failed (4 distinct tests), 27 ignored, 387 binaries | 74 min |
| `docs` | Linux | success | 35 s |
| `release-lint` | Linux | success | 53 s |
| `build-windows` | Windows | success | 2 min 4 s |
| `test-windows` | Windows | failure (`continue-on-error: true`): 3373 passed, 479 failed (476 distinct names), 11 ignored, 387 binaries, no hang | 35 min |

## Classes

- (a) a test that is on `crates/horch-e2e/expected-failures/`
- (b) a compile error on that OS
- (c) a test that is unix-only by its code but runs on Windows
- (d) a real fault of the product (or of the test harness) on that OS
- (e) a fault of the CI setup
- (f) the test is stale: it expects a text or a command that fleetd changed

## Jobs of `ci.yml`

| Job | OS | Limit |
|---|---|---|
| `test` (matrix) | `ubuntu-latest`, `macos-15` | `timeout-minutes: 120` |
| `docs` | `ubuntu-latest` | none |
| `release-lint` | `ubuntu-latest` | none |
| `build-windows` | `windows-latest` | none |
| `test-windows` | `windows-latest`, `continue-on-error: true` | `timeout-minutes: 120` |

## macOS

Job `test (macos-15)`: failure. 5338 tests passed, 6 failed, 26 ignored. Run 5: 5270 passed, 9 failed. The 6 failure lines are 5 distinct tests: `skl_06_e2e_marketplace_offline` is counted twice, because `git_env_cannot_reach_another_repository` runs it again as a child process.

| Test | Class | First error line | file:line of the cause |
|---|---|---|---|
| `skl_06_e2e_marketplace_offline` (**new red**, also Linux) | d | `the test fleetd exited (Some(ExitStatus(unix_wait_status(256)))): horch: horch fleetd: refused: could not run `/nonexistent version`: No such file or directory (os error 2). The rescue of a worktree saves its files with the filters off, and that needs git 2.41 or newer` | The test sets `HORCH_GIT_BIN=/nonexistent` (`crates/horch-e2e/tests/skills_exposure.rs:329`) and then launches a worker; the launch makes the test fleetd serve, and the start check of the git version (`513b73e1`, T-389) refuses. Panic: `crates/horch-e2e/src/fleetd.rs:105`. Green in run 5: the cause of the change is not proved (probable: T-384 or `cb3a6552`, which changed which launches start a fleetd in the world). |
| `git_env_cannot_reach_another_repository` (**new red**, also Linux) | d | the child run of `skl_06_e2e_marketplace_offline` fails (same message as the row above) | `crates/horch-e2e/tests/skills_exposure.rs:635` (the assert on the child output). Same cause as the row above. It was red on Windows in run 5. |
| `fleetd_review_spawn_does_not_use_the_spawn_kind` (**new red**) | d | `the spawn wrote the task files of its attempt` with stderr `horch spawn refused: new dispatch is on hold until the machine recovers: 1-minute load 16.37 is at or above 12.00 (4 for each of 3 CPUs) (health_max_load_per_cpu)` | `crates/horch/tests/p6_retired_worker_commands.rs:605`. The test runs `horch spawn` with the health guard on: a loaded runner (3 CPUs, load 16) holds dispatch. Rule 6: a test that depends on the load of the machine is a bug. |
| `phase4::ask_scenario_sends_one_question` (**new red**) | d | ``assertion `left == right` failed: [Object {"argv": ["--version"], "call_id": ...`` with `left: Null`, `right: 0` | `crates/horch-e2e/tests/fakes.rs:1247` (`calls.last()` is the `--version` call of a fleetd conformance job, not the `agy` agent call). The same fault as FIX-29 (T-334) fixed in `skills_exposure.rs`: a test counts a conformance launch as the agent. Not proved by a rerun. |
| `g50_legacy_orchestrator_and_host_load_are_advisory` (**new red**) | d? | `timed out waiting for the launch of the worker` (after 140 s) | `crates/horch-e2e/tests/fleet18_observability.rs:394` (`wait_up_to`, `LAUNCH_WAIT`). The cause is not proved: load of the runner or the conformance hold. The other 21 tests of the file are green. |

Total: 5 distinct tests (6 failure lines). All 9 tests of run 5 are green now: the 4 `restore_*` tests of `p3_worktrees_cli.rs`, `g56_later_spawn_receives_recorded_fact`, `arc_14_provenance_on_spawn_substitute_resume`, the 2 `the_hermetic_story` tests and `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing`.

## Linux

Job `test (ubuntu-latest)`: failure. 5340 tests passed, 5 failed (4 distinct tests), 27 ignored. Run 5: 5272 passed, 8 failed.

| Test | Class | First error line | file:line of the cause |
|---|---|---|---|
| `the_whole_flow_on_the_fake_herdr` (still red) | d? | `timed out waiting for T-3 done` | `crates/horch-e2e/tests/taskledger.rs:293` (the wait helper). T-400 owned it and is done after the start of this run: its work is not on the tip of run 6. The expected-failures entry is gone (T-289, `c426be26`). |
| `skl_06_e2e_marketplace_offline` (**new red**, also macOS) | d | `the test fleetd exited ... refused: could not run `/nonexistent version`` | see macOS |
| `git_env_cannot_reach_another_repository` (**new red**, also macOS) | d | the child run fails | see macOS |
| `fm36_fleetd_restart_in_a_bundle_and_its_bisect_builds_nothing_twice` (**new red**, Linux only) | d? | ``assertion `left == right` failed: fleet/r1-T1-a`` with `left: "in_bundle"`, `right: "landed"`. The evidence: bundle 1 is `building` with `job=9`, and every job (1 to 10) is `queued`. | `crates/horch-e2e/tests/p4_landing.rs:507` (`assert_landed_but_b`, which runs after the restart of fleetd). The jobs did not run: the bundle build was still queued when the test asserted. The cause is not proved. 1 failure in 1 run: rule 6 applies. It is green on macOS. |

Total: 5 failure lines, 4 distinct tests. Of the 8 tests of run 5: 7 are green now (`g56_...`, `arc_14_...`, the 2 hermetic stories, `spawn_fails_...`, `arc_26_e2e_lifecycle_matrix_claude`, `rr4_file_request_and_job_events_wake_pass_before_period`), and `the_whole_flow_on_the_fake_herdr` is red.

## Windows

Job `build-windows` (`cargo build --workspace --locked`): success. No compile error (class b: none).

Job `test-windows` (`cargo test --workspace --locked --no-fail-fast`): failure, as `continue-on-error: true` allows. It started 18:32Z and ended 19:07Z. 3373 passed, 479 failed (476 distinct names), 11 ignored in 387 binaries (run 5: 358 binaries, 3133 passed, 562 failed, then a hang). All 387 binaries ran.

### The hang of run 5 is gone

`windows::channel::tests::client_that_never_reads_the_reply_does_not_block_the_next_client` is `ok` (T-407). The 6 tests of pipe error 233 (`fleetd_channel.rs`) are green. The 27 binaries after `horch-platform` ran for the first time. They hold 9 red tests, listed in the table of new red tests below.

### Groups of the 479 failures

Each failure block went to the first rule that matched its text. The counts are failure blocks (478).

| Group | Count (run 5) | Class | First error line | file:line of the cause |
|---|---|---|---|---|
| `git` runs only in a sandbox, and Windows has none (FIX-37, open operator decision OD-7) | 203 (98) | d | `no sandbox: sandbox is not supported on windows: Windows has no Seatbelt and no bubblewrap; fleetd runs git in a project only in the sandbox, so the job ran no git` | `crates/horch-core/src/fleetd/integration/application/commands.rs:407`. Files: `w3_finish_steps.rs` 38, `worktree_jobs.rs` 17+, `bundles.rs` 15, `integration_git.rs` 14, `idea_direct.rs` 12, `ref_move.rs` 9, `worktree_create.rs` 7, `worktree_jobs_stop.rs` 7, `bisect.rs` 6, `branch_abandon.rs` 6, `bundle_build_job.rs` 6, `feature_abandon.rs` 6, `merged_with_ideas.rs`, `new_features.rs`, `stacks.rs` 6 each. 99 of the 203 are tests of the verbatim path group of run 5: T-406 fixed the path, and the test then reaches the sandbox refusal. |
| The launch refuses: `Claude Code has no sandbox on windows` (FIX-37, same decision) | 57 (54) | d | ``called `Result::unwrap()` on an `Err` value: sonnet: refusing to launch without a sandbox: Claude Code has no sandbox on windows`` | `crates/horch-core/src/execution/lifecycle.rs`, `harness/claude.rs`, `p5_claude_sandbox.rs`, `intake_roster.rs`. `p7_conformance.rs` 17, `launch.rs` 15 |
| The test fleetd refuses: `git not found on PATH` (**new group**) | 14 (0) | c | `the test fleetd exited (exit code: 1) before it served its channel: horch fleetd: refused: git not found on PATH ... needs git 2.41 or newer` | `crates/horch/tests/support/fleetd.rs:92` (`git_on_real_path` joins `git`, not `git.exe`: 6 tests, `ctx_*`) and `crates/horch-e2e/src/harness.rs:1572` (the fake git 2.41 of `cb3a6552` is a `sh` script: 7 tests of `crates/horch-e2e/src/fleetd.rs:105`, and `p0_fleetd_hang.rs` 1). Both come from the fix of the start check (T-399 in T-384, T-397). |
| Verbatim path `\\?\C:\...` or a `worktree_add` job that stays open | 18 (154) | d | `worktree \\?\C:\Users\runneradmin\...` and ``git ["read-tree", ...]: Unable to create '\\?\C:\...' `` | `crates/horch-core/tests/feature_states.rs`, `stacks.rs`, `trial_merges.rs`, `landing_states.rs`. See "T-406" below. |
| Unix-only by its code, runs on Windows | 14 + 3 | c | `video-produce/scripts/lint_notes.py has no execute bit` (1); `the health guard processes does not hold dispatch: not supported on this platform` (3); `skills_exposure.rs` 11 (the test uses a `sh` fake or a unix path); `private CODEX_HOME` 3 (`messaging.rs:147`) | `crates/horch-core/tests/video_pipeline_wave.rs`, `crates/horch/src/cmd/taskledger_spawn.rs`, `crates/horch-e2e/tests/skills_exposure.rs`, `fleet_start.rs`, `messaging.rs:147` |
| Unix path or Windows path text in an assertion | 8 + 18 | c | `XDG_STATE_HOME: ... '/xdg-state\horch'`; ``assertion failed: first.starts_with(w.root.join("state/skills/tdd"))`` | `crates/horch/src/cmd/recipes.rs`, `smoke.rs`, `crates/horch-core/src/execution/store.rs`, `harness/claude.rs`, `harness/launch.rs`, `fleetd/task_execution/domain/outbox.rs` (`handoff ... must be an absolute path`), `runtime/process.rs`, `crates/horch/src/cmd/install.rs`, `crates/horch/tests/skills_cli.rs`, `complexity.rs` |
| The row of a tree or a tip is missing (git did not run: probable the same cause as the sandbox group) | 15 | d? | `called Result::unwrap() on an Err value: QueryReturnedNoRows` (`feature_states.rs` 6, `i308_triage_apply.rs` 5); `no remote_tips row of main` (`fetch_job.rs` 4) | `crates/horch-core/tests/feature_states.rs`, `i308_triage_apply.rs`, `fetch_job.rs:450` |
| `git config --list` fails in a temp repo | 6 | d? | `feature F-0001 Caused by: 0: running git config --list --show-origin --show-scope -z in C:\Users\runneradmin\...\repo` | `crates/horch-core/tests/merged_with_ideas.rs` |
| Access denied (os error 5) | 15 (13) | d? | `PermissionDenied ... marketplace.lock: Access is denied. (os error 5)` | `crates/horch-marketplace/tests/marketplace.rs` (14 failures), `harness/trust.rs` 6, `catalog.rs` 4 |
| `timed out after 120 s` | 8 (8) | d | `timed out after 120 s waiting for the worktree of the task` (`g69_drop_*`), `... waiting for the START.md prompt of the worker` (`g56_...`) | `crates/horch-e2e/src/harness.rs` (5), `fleet18_observability.rs` (3). Probable cause: the sandbox group. |
| The owner SID of the idea file is not the ledger owner | 7 (7) | d? | `Rejected { file: "idea.md", reason: "owner sid S-1-5-32-544 is not the ledger owner sid ..." }` | `crates/horch-core/tests/idea_intake.rs:125`, `idea_waiting.rs`, `fleetd_handlers_idea.rs`, `fakes.rs` |
| `%1 is not a valid Win32 application` | 5 (5) | c | `%1 is not a valid Win32 application` | `crates/horch-core/tests/fleetd_pass.rs` 5 (a `sh` script that the test starts), `fleet18_observability.rs` |
| `horch-platform` tests that ran for the first time | 5 | d | `the_listener_accepts_a_next_client_after_the_first`: `Kind(TimedOut)`; `file_identity_differs_for_2_files_and_is_stable_for_1`: `left != right`; 3 tests of `service.rs`: the fake service manager gets no call (`left: []`) | `crates/horch-platform/tests/channel.rs`, `fs.rs`, `service.rs` |
| Other, 1 or a few tests each | about 40 | d? | `telemetry_collect.rs` 6 (`rec-b1: -0`, `0.1498326`), `roster` 6, `catalog.rs` 4, `context` 4, `p5_restore_warning.rs` 1, `doctor_extras_*` 3 and `g54_doctor_warns_on_proxy_host` 1 (`delivery test failed: herdr did not answer inside the push budget`) | the log of the job is the source |
| The CRLF text of a docs copy | 1 (4) | d | `left: "# Guide\r\n" right: "# Guide\n"` | `crates/agent-docs-sync/tests/changes.rs` (`also_git_copies_the_docs_folder_from_a_local_repository`) |

The groups overlap by small counts, because I read the first message of a block. Use the counts as sizes.

How many failed tests of run 5 are green now (by test name): run 5 had 558 distinct failed names. Run 6 has 476.

| Of the failed tests of run 5 | Count |
|---|---|
| green now | 102 |
| still red | 455 |
| not run now | 1 |
| red now, and `ok` in run 5 | 12 |
| red now, and not in run 5 at all (binaries that ran for the first time) | 9 |

## Job `docs`

Success (run 5: 6 rustdoc errors). T-398 fixed them.

## Job `release-lint` (`cargo clippy --workspace --bins --release --locked -- -D warnings`)

Success.

## Difference to run 5

CI5 = run 44890393381 (head `05ff4081`). CI6 = this run (head `2906f9cc`).

### Per job

| Job | CI5 | CI6 | Change |
|---|---|---|---|
| `test (macos-15)` | 5270 passed, 9 failed | 5338 passed, 6 failed (5 tests) | better: 9 of 9 green, 5 new red |
| `test (ubuntu-latest)` | 5272 passed, 8 failed | 5340 passed, 5 failed (4 tests) | better: 7 of 8 green, 3 new red |
| `docs` | failure, 6 errors | success | **fixed** (T-398) |
| `release-lint` | success | success | no change |
| `build-windows` | success | success | no change |
| `test-windows` | cancelled by the limit of 120 min; 3133 passed, 562 failed; 358 binaries | failure after 35 min; 3373 passed, 479 failed; 387 binaries | the hang is gone; 102 of 558 names are green; 21 new red |

### Each fix of the wave

| Fix | State now |
|---|---|
| T-384: 13 e2e tests of a world with no git (conformance hold, fake git 2.41) | **green** on macOS and Linux (`p0_liveness` and the polite-stop tests are not in the failures). **New red on Windows**: 14 tests (`git not found on PATH`: the fake git is a `sh` script and the real-PATH lookup misses `.exe`) |
| T-397: stale refusal texts and the 4 `restore_*` tests (macOS) | **green**: the 2 hermetic stories, `arc_14_...`, `spawn_fails_...`, 4 `restore_*`, `g56_...` are `ok` on macOS and Linux. On Windows `g56_...` is red (`timed out after 120 s waiting for the START.md prompt of the worker`: the sandbox group). |
| T-398: rustdoc | **green** (`docs` job success) |
| T-407: Windows channel hang and pipe error 233 | **green**: the hang test is `ok`, the 6 tests of error 233 are green, the job ends in 35 min. 5 tests of `horch-platform` that ran for the first time are red (table below) |
| T-406: Windows verbatim paths, the owner of the job result file | **mostly green**: the group of run 5 (154 tests) is 18 now. 10 are green by name, 99 moved to the sandbox group (the path is right now and git has no sandbox), 24 moved to other groups |
| T-409: Windows path and text forms, transcript lookup | **partly green**: `taskledger/map.rs` (10), `golden_prompts.rs` (6), `context_roster.rs` (7), `compact` (6), the 4 CRLF roster and frontmatter tests are green. Still red: `telemetry_collect.rs` 6, `roster` 6, `context` 6, `harness/trust.rs` 6, `marketplace.rs` 14 |
| T-404: project facts of a push attempt | **green** on macOS and Linux (`g56_later_spawn_receives_recorded_fact` is `ok`) |
| T-289: gate tree, `the_whole_flow_on_the_fake_herdr` | **still red on Linux** (`timed out waiting for T-3 done`); green on macOS. T-400 (done later) has the fix of the role race; its open product fault has no ticket |

### New red tests: regressions of the fix wave first

| Test | OS | Cause | Commit or task |
|---|---|---|---|
| `skl_06_e2e_marketplace_offline` | macOS, Linux | the test fleetd refuses to start with `HORCH_GIT_BIN=/nonexistent` | T-389 check at the fleetd start (`513b73e1`), reached by a launch after T-384 or `cb3a6552`. **No ticket** |
| `git_env_cannot_reach_another_repository` | macOS, Linux | the child run of the test above | **No ticket** (it was Windows-only red in run 5) |
| `fleetd_review_spawn_does_not_use_the_spawn_kind` | macOS | `health_max_load_per_cpu` holds the spawn at load 16 on 3 CPUs | **No ticket** |
| `phase4::ask_scenario_sends_one_question` | macOS | the last `antigravity` call is the `--version` call of a conformance job | **No ticket** |
| `g50_legacy_orchestrator_and_host_load_are_advisory` | macOS | no worker launch in 140 s; not proved | **No ticket** |
| `fm36_fleetd_restart_in_a_bundle_and_its_bisect_builds_nothing_twice` | Linux | the bundle build job stays queued; not proved | **No ticket** |
| Windows: 14 tests `git not found on PATH` (`ctx_01_cli_table_from_fixture_home`, `ctx_21_compact_at_sets_base`, `ctx_21_substituted_launch_takes_the_fallback_compact_at`, `fm12_hung_fleetd_is_restarted_on_each_os`, and 10 more) | Windows | `git_on_real_path` has no `.exe`; the fake git is a `sh` script | `cb3a6552` (T-399), `crates/horch/tests/support/fleetd.rs` (T-397). **No ticket** |
| Windows: `bundled_skill_has_a_stable_content_addressed_path`, `a_roster_in_a_linked_worktree_maps_to_the_main_checkout`, `hdf_02_claude_settings_default_yields_to_operator_chain`, `hdf_02_codex_default_yields_to_operator_config_unless_forced`, `failed_conformance_adds_1_idea_with_the_diff_and_the_checks`, `missing_gh_gives_the_cause_in_the_job_result`, `teammates_check_warns_about_a_copied_messages_base_and_passes` | Windows | path text (`\` against `/`) or the Windows runner; the cause of each is the first line of its block | after T-406 or T-409; **no ticket** |
| Windows: `doctor_is_quiet_when_no_offered_teammate_uses_such_a_harness`, `doctor_warns_for_a_harness_the_watchdog_cannot_see`, `doctor_warns_when_the_codex_reviewer_has_no_codex`, `g54_doctor_warns_on_proxy_host` | Windows | `warning: delivery test failed: herdr did not answer inside the push budget` and `resume_agents_on_restore is not false` appear in the doctor output; the tests expect a quiet output | **No ticket** |
| Windows, `horch-platform` (first run of the binaries): `the_listener_accepts_a_next_client_after_the_first` (`Kind(TimedOut)`, `tests/channel.rs`), `file_identity_differs_for_2_files_and_is_stable_for_1` (`tests/fs.rs`), 4 tests of `tests/service.rs` (`ensure_*`, `restart_calls_the_manager_once`, `a_ledger_label_names_only_its_own_files_and_manager_calls`: the fake manager gets no call) | Windows | d: product or test of the Windows platform | T-407 fixed the unit test that hung; **no ticket** for these 6 |
| Windows: `launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out`, `merged_feature_releases_its_gate_tree` | Windows | a record of the process or a gate tree row is missing (the sandbox group) | **No ticket** |

### Tasks that cover each failure that is still red

State of the run board at 20:08Z (`horch ledger status --run fleetd-finish`): 423 tasks, 370 done, 1 running (this task), 1 landing (T-94), 8 queued (T-221 to T-228: the end gates and the pull request), 43 cancelled. T-400, T-408, T-413 and T-416 are done. The tip of this run (`2906f9cc`) holds none of the work of T-400, T-413 and T-94 that landed after the run started: check `git log` before you read a row below.

| Failure | Task |
|---|---|
| `the_whole_flow_on_the_fake_herdr` (Linux) | T-400 (ticket 258, done after the start of this run; its log of 19:14Z and 19:33Z names a role number race, fixed in `f1d34fdb`, and an **open product fault: a spawn while fleetd is down writes a spool request and then finds no record**). The open fault has **no ticket** |
| Windows: project folder with an old ACL, or a stored verbatim project root (the verbatim group that stays, 18) | T-418 (delta review 3, F1 and F2; named in `decisions.md` 19:00Z; not read by me as a ticket) |
| `rr4_...` and `arc_26_...` (Linux, red in run 5) | T-413 (ticket 264, done): both green in this run |
| The 7 expected-failures rows | T-94 (landing; its audit of 19:16Z: 6 rows green 3 of 3, 1 test deleted; `i308_fleet_steward.txt` keeps 3 entries = an operator decision) |
| Windows sandbox for git (203) and the launch refusal (57): 260 tests | the operator decision OD-7 / FIX-37 (no task) |
| `skl_06_e2e_marketplace_offline`, `git_env_cannot_reach_another_repository` | no ticket |
| `fleetd_review_spawn_does_not_use_the_spawn_kind`, `phase4::ask_scenario_sends_one_question`, `g50_legacy_orchestrator_and_host_load_are_advisory`, `fm36_fleetd_restart_in_a_bundle_and_its_bisect_builds_nothing_twice` | no ticket |
| Windows: the 14 `git not found on PATH` tests | no ticket |
| Windows: the 9 new tests of `horch-platform` and the 12 tests that were `ok` in run 5 | no ticket |
| Windows: 15 `QueryReturnedNoRows` rows, 6 `git config --list`, 15 access denied, 8 timed out, 7 owner SID, 5 Win32, 14 unix-only, 26 path text, `telemetry_collect.rs` 6, the rest | no ticket |

### The 5 largest causes that have no ticket

1. **Windows has no sandbox for git or for the launch (FIX-37 / OD-7): 260 tests** (203 + 57). It is an operator decision with no task. The next largest groups depend on it: about 40 more tests (15 missing rows, 8 timeouts, 6 `git config`, `Running` jobs) have the same probable cause.
2. **Tests that are unix-only by their code and run on Windows (class c): about 45 tests.** `sh` scripts (fake git, `fleetd_pass.rs` 5, `Win32 application` 5), execute bits, the health guard text, `private CODEX_HOME` 3, `skills_exposure.rs` 11 and the 14 tests of the `git not found on PATH` group. The fix is `#[cfg(unix)]` or a Windows form.
3. **Windows path and text forms in test expectations: about 40 tests.** `/` against `\` (26 in the group "path text"), `telemetry_collect.rs` 6, `roster` 6, `context` 6.
4. **Windows file access and identity: about 45 tests.** Access denied (15: `marketplace.lock`, `trust.rs`, `catalog.rs`), owner SID 7 (the runner is an administrator), `horch-platform` 6 (file identity, the service manager fake, the listener after the first client), 12 tests that were `ok` in run 5.
5. **The e2e tests that depend on the machine (macOS and Linux): 6 tests in 6 files** (`the_whole_flow_on_the_fake_herdr` has T-400 and is not counted). `fleetd_review_spawn_does_not_use_the_spawn_kind` (health guard at load 16), `g50_...` (launch wait), `fm36_...` (bundle build job queued), `ask_scenario_sends_one_question` (conformance call), and the 2 `skills_exposure.rs` tests (the start check of git). They are the only red tests of macOS and Linux that have no ticket. 3 of them were green in run 5.

### `no ticket` rows of run 5 and their state now

| Row of run 5 | State now |
|---|---|
| 6 `docs` errors | **fixed** (T-398) |
| `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing` | **fixed** (T-397) |
| The 4 `restore_*` tests (macOS) | **fixed** (T-397: the git of the real PATH) |
| `g56_...`, `arc_26_...`, `rr4_...` | **fixed** on macOS and Linux (T-397, T-404, T-413); `g56_...` is red on Windows (sandbox group) |
| Windows: the hang of `horch-platform` | **fixed** (T-407) |
| Windows: 2 CRLF parser tests, `git_env_cannot_reach_another_repository` (Windows) | CRLF **fixed**; `git_env_...` is red on macOS and Linux now |
| Windows: verbatim path (157) | **mostly fixed** (T-406): 18 left; 99 moved to the sandbox group |
| Windows: sandbox for git (160) | **open operator decision**: 260 tests now (203 + 57) |

## Fix units

Only failures with no ticket. Each unit is for 1 worker. I did not run any command (ticket rule). A command is for macOS through `scripts/dev-sandbox.sh`. A unit says "no macOS repro" when the failure is Windows-only or Linux-only.

| Unit | Fixes | Files it touches | Command that reproduces 1 failure |
|---|---|---|---|
| FIX-50: the git start check in the 2 skills tests | `skl_06_e2e_marketplace_offline`, `git_env_cannot_reach_another_repository` (macOS, Linux) | `crates/horch-e2e/tests/skills_exposure.rs:329` (do not set `HORCH_GIT_BIN` to a path that fleetd refuses: give the launch a fake git 2.41 and make the test prove "no git from here on" another way), `crates/horch-e2e/src/fleetd.rs:105` if the harness must start fleetd with the original git | `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test skills_exposure skl_06_e2e_marketplace_offline` |
| FIX-51: tests that depend on the load or the conformance job | `fleetd_review_spawn_does_not_use_the_spawn_kind`, `phase4::ask_scenario_sends_one_question`, `g50_legacy_orchestrator_and_host_load_are_advisory` (macOS). Run each 3 times first (rule 6). | `crates/horch/tests/p6_retired_worker_commands.rs:605` (set the health limits off or high in the world of the test), `crates/horch-e2e/tests/fakes.rs:1247` (find the call of the `agy` agent: skip the `--version` calls of a conformance job, as FIX-29 does), `crates/horch-e2e/tests/fleet18_observability.rs:394` (give the wait the context: the launch log and the calls of `claude`) | `scripts/dev-sandbox.sh cargo test -p horch --locked --test p6_retired_worker_commands fleetd_review_spawn_does_not_use_the_spawn_kind`; `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test fakes phase4::ask_scenario_sends_one_question`; `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test fleet18_observability g50_legacy_orchestrator_and_host_load_are_advisory` |
| FIX-52: `fm36` on Linux | `fm36_fleetd_restart_in_a_bundle_and_its_bisect_builds_nothing_twice` (cause not proved; run it 3 times on Linux) | `crates/horch-e2e/tests/p4_landing.rs:507` and the wait before it (wait for the end of the bundle build job, not for the restart) | no macOS repro (green on macOS). On Linux: `cargo test -p horch-e2e --locked --test p4_landing fm36_fleetd_restart_in_a_bundle_and_its_bisect_builds_nothing_twice` |
| FIX-53: Windows, the fake git and the `.exe` lookup | the 14 tests of `git not found on PATH` | `crates/horch/tests/support/fleetd.rs:92` (try `git.exe`, or use the `PATHEXT`), `crates/horch-e2e/src/harness.rs:1572` (a fake git for Windows, or `#[cfg(unix)]` on the tests that need the script) | no macOS repro. A check on macOS that the change keeps green: `scripts/dev-sandbox.sh cargo test -p horch --locked --test context_cli` |
| FIX-54: Windows, the 9 new tests of `horch-platform` and 12 tests that were `ok` in run 5 | `the_listener_accepts_a_next_client_after_the_first`, `file_identity_differs_for_2_files_and_is_stable_for_1`, 4 tests of `service.rs`, the 4 `doctor_*` tests, `bundled_skill_has_a_stable_content_addressed_path`, `a_roster_in_a_linked_worktree_maps_to_the_main_checkout`, `hdf_02_*` (2), `failed_conformance_adds_1_idea_with_the_diff_and_the_checks`, `missing_gh_gives_the_cause_in_the_job_result`, `teammates_check_warns_about_a_copied_messages_base_and_passes` | `crates/horch-platform/tests/channel.rs`, `fs.rs`, `service.rs`, `crates/horch-e2e/tests/doctor_extras_watchdog.rs`, `doctor_extras_reviewer.rs`, `crates/horch/tests/skills_path.rs`, `crates/horch/src/cmd/install.rs`, `crates/horch-core/tests/harness_defaults.rs`, `p7_drift_idea.rs`, `fetch_job.rs`, `crates/horch/tests/context_policy_check.rs` | no macOS repro (Windows-only). `scripts/dev-sandbox.sh cargo test -p horch-platform --locked --test channel` builds on macOS and shows the test is not unix-only |
| FIX-55: Windows, tests that are unix-only by their code | `skills_exposure.rs` 11, `fleetd_pass.rs` 5, `Win32 application` 5, `private CODEX_HOME` 3, health guard 3, `video_pipeline_wave.rs` 1 | `crates/horch-e2e/tests/skills_exposure.rs`, `crates/horch-core/tests/fleetd_pass.rs`, `crates/horch-e2e/tests/messaging.rs:147`, `crates/horch/src/cmd/taskledger_spawn.rs`, `crates/horch-core/tests/video_pipeline_wave.rs` (`#[cfg(unix)]`, or a Windows form) | no macOS repro |
| FIX-56: Windows, path and text forms | the group "path text" (26), `telemetry_collect.rs` 6, `roster` 6, `context` 6, `harness/trust.rs` 6, `marketplace.rs` 14, 1 CRLF test in `agent-docs-sync` | `crates/horch/src/cmd/recipes.rs`, `smoke.rs`, `context.rs`, `crates/horch-core/src/execution/store.rs`, `harness/claude.rs`, `harness/launch.rs`, `fleetd/task_execution/domain/outbox.rs`, `crates/horch-core/tests/telemetry_collect.rs`, `crates/horch-marketplace/tests/marketplace.rs`, `crates/agent-docs-sync/tests/changes.rs` | no macOS repro. A check on macOS that the change keeps green: `scripts/dev-sandbox.sh cargo test -p horch-core --locked --lib harness::trust` |

Not a unit: FIX-37 / OD-7 (the 260 tests with no sandbox for git or for the launch on Windows, and about 40 tests with the same probable cause). It stays an operator decision: a sandbox for Windows, or `#[cfg(unix)]` on these tests.

Order: FIX-50 and FIX-51 first: they are the only red tests of macOS and Linux that have no ticket. FIX-52 needs a diagnosis first. FIX-53 before the next Windows run, because 14 tests fail by the harness and not by the product. The end gates T-221 to T-228 need a green macOS and Linux job: after T-400, FIX-50 to FIX-52, run the CI again.
