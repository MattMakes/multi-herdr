# CI run 5 of fleetd, 2026-11-21

- Run: 44890393381, https://github.com/MattMakes/multi-herdr/actions/runs/44890393381
- Trigger: `workflow_dispatch` on `fleet/fleetd-finish`, started 2026-11-21 09:36Z, ended 11:36Z. Result: `failure`.
- Head commit of the run: `05ff408109f34d10dce7df94084c9b8979cb2cc9` (T-335 comment restore). The tip of `origin/fleet/fleetd-finish` at the start and the local branch were the same commit. The check "headSha equals the tip" holds. 118 commits landed between the head of run 4 (`2c6ef0b4`) and this head.
- Compared with: run 4, `docs/specs/fleetd/gates/ci-run-4.md` (run 44861461538, head `2c6ef0b4`).
- Source of the counts: the lines `test result:` of each job log, the lines `test <name> ... FAILED`, and the blocks `---- <test> stdout ----`. For Windows I also compared the test names of the log of run 4 with this log. I ran no test locally (ticket rule).
- Run result: 2 of 6 jobs passed (run 4: 2 of 6). 3 jobs failed. `test-windows` ended at its `timeout-minutes: 120` (new from T-342): the first hang that the limit ended.

| Job | OS | Result | Time |
|---|---|---|---|
| `test (macos-15)` | macOS | failure: 5270 passed, 9 failed, 31 ignored | 85 min 48 s |
| `test (ubuntu-latest)` | Linux | failure: 5272 passed, 8 failed, 32 ignored | 68 min 3 s |
| `docs` | Linux | **failure**: 6 rustdoc errors (run 4: 1 error, other file) | 30 s |
| `release-lint` | Linux | success | 38 s |
| `build-windows` | Windows | success | 2 min 24 s |
| `test-windows` | Windows | **cancelled by the limit of 120 min** (`continue-on-error: true`): 3133 passed, 562 failed, 7 ignored in 358 test binaries, then a hang in 1 unit test of `horch-platform` | 120 min |

## Classes

- (a) a test that is on `crates/horch-e2e/expected-failures/`
- (b) a compile error on that OS
- (c) a test that is unix-only by its code but runs on Windows
- (d) a real fault of the product (or of the test harness) on that OS
- (e) a fault of the CI setup
- (f) the test is stale: it expects a text or a command that fleetd changed

A cause that the log does not name in the failure line is marked "probable" or "not proved".

## Jobs of `ci.yml`

| Job | OS | Limit |
|---|---|---|
| `test` (matrix) | `ubuntu-latest`, `macos-15` | `timeout-minutes: 120` |
| `docs` | `ubuntu-latest` | none |
| `release-lint` | `ubuntu-latest` | none |
| `build-windows` | `windows-latest` | none |
| `test-windows` | `windows-latest`, `continue-on-error: true` | `timeout-minutes: 120` |

## macOS

Job `test (macos-15)`: failure. 5270 tests passed, 9 failed, 31 ignored. Run 4: 5186 passed, 23 failed.

| Test | Class | First error line | file:line of the cause |
|---|---|---|---|
| `restore_of_a_task_makes_branch_rescue_t_seq_n_and_its_tree_with_each_rescued_file`, `restore_of_a_task_whose_tree_is_removed_uses_its_rescue_ref`, `restore_refuses_when_the_branch_or_the_folder_exists_and_changes_nothing`, `restore_runs_no_hook_of_the_project` (4) | d? | `timed out waiting for fleetd to rescue the tree of T1` | `crates/horch/tests/p3_worktrees_cli.rs:153` (the `wait_for` helper). Red in run 4 too. T-290 (`686940dd`, `worktree_list_period_s = 5`) landed in this head and did not fix it. The cause is not proved. |
| `g56_later_spawn_receives_recorded_fact` (**new red**, also Linux) | d? | `timed out after 120 s waiting for worker brief with project facts` | `crates/horch-e2e/tests/fleet18_brief_worktree.rs:380` (the wait for a `claude` call that holds the fact; the wait gives no context). The cause is not proved. |
| `arc_14_provenance_on_spawn_substitute_resume` (**new red**, also Linux) | f | `timed out after 120 s waiting for the refusal of the researcher launch`; the ledger row holds `end_reason: "its spawn failed: REFUSED: researcher cannot start. claude exhausted (7d 100%, resets Fri 14:00Z)..."` | `crates/horch-e2e/tests/routing_provenance.rs:139-143` waits for the text `launch_failed: REFUSED: researcher cannot start...`. The prefix `its spawn failed: ` is new: `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs:192` (`let why = format!("its spawn failed: {}", p.reason)`), commit `d8a08e35` (T-254, CI FIX-25). |
| `the_hermetic_story`, `the_hermetic_story_with_an_ollama_that_starts_in_7_s` (**new red**, also Linux) (2) | f | `timed out after 120 s waiting for the refusal of the launch` (the log shows steps PASS 1 to PASS 7, then the wait; the row holds `its spawn failed: REFUSED: opus cannot start...`) | `crates/horch-e2e/tests/scenario.rs:189-195` waits for `launch_failed: REFUSED: opus cannot start.`. The same cause as the row above (`d8a08e35`). |
| `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing` (**new red**, also Linux) | f | `{"...,"end_reason":"its spawn failed: no brief at /tmp/horch-e2e-startup-erro-.../herdr-orchestration-elsewhere/sonnet-1.brief.json (spawn this worker with `horch spawn`)...","status":"launch_failed"}` | `crates/horch-e2e/tests/startup_error.rs:224` (`reason.starts_with("no brief at ")`). The same cause (`d8a08e35`). |

Total: 4 + 1 + 1 + 2 + 1 = 9.

## Linux

Job `test (ubuntu-latest)`: failure. 5272 tests passed, 8 failed, 32 ignored. Run 4: 5157 passed, 52 failed.

| Test | Class | First error line | file:line of the cause |
|---|---|---|---|
| `g56_later_spawn_receives_recorded_fact` | d? | `timed out after 120 s waiting for worker brief with project facts` | `crates/horch-e2e/tests/fleet18_brief_worktree.rs:380` (see macOS) |
| `arc_14_provenance_on_spawn_substitute_resume` | f | `timed out after 120 s waiting for the refusal of the researcher launch` | `crates/horch-e2e/tests/routing_provenance.rs:139-143` (see macOS) |
| `the_hermetic_story`, `the_hermetic_story_with_an_ollama_that_starts_in_7_s` (2) | f | `timed out after 120 s waiting for the refusal of the launch` | `crates/horch-e2e/tests/scenario.rs:189-195` (see macOS) |
| `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing` | f | `{"...,"end_reason":"its spawn failed: no brief at /var/tmp/horch-e2e-startup-erro-.../sonnet-1.brief.json ...` | `crates/horch-e2e/tests/startup_error.rs:224` (see macOS) |
| `arc_26_e2e_lifecycle_matrix_claude` (**new red**, Linux only) | d? | `timed out after 120 s waiting for the attempt to be live`; the context lists only `--version` calls of `antigravity`, `claude`, `codex` | `crates/horch-e2e/tests/lifecycle.rs:209` (the wait for `attempt_state == "live"`). The cause is not proved. It passes on macOS in this run. |
| `rr4_file_request_and_job_events_wake_pass_before_period` (**new red**, Linux only) | d? | `after 15 s: a commit to the ledger wakes a pass of fleetd PID 261802 before its period of 300 s` | `crates/horch-e2e/tests/p1_pass.rs:106` (`wait_until_or`). T-71 moved this test to the push flow. It is green on macOS in this run and in run 4. 1 failure in 1 run: rule 6 applies, find the cause. |
| `the_whole_flow_on_the_fake_herdr` (class a) | a, d? | `timed out waiting for T-3 done`, then `crates/horch-e2e/src/expected.rs:108`: the list gives the reason `the gate tree gate-F-0001 stays open` and the test fails with another message | `crates/horch-e2e/tests/taskledger.rs:293` (the wait helper). Ticket 258 (T-400, queued) covers the timeout: "the whole flow test burns 2 of 3 attempts of each review task". The list entry then needs a new reason or its removal (T-94, queued). |

Total: 8. The 10 tests of the group "pass helper with an empty PATH" of run 4 are green.

## Windows

Job `build-windows` (`cargo build --workspace --locked`): success.

Job `test-windows` (`cargo test --workspace --locked --no-fail-fast`): **cancelled at 11:36:38Z by `timeout-minutes: 120`**. The step started 09:36:57Z. 3133 passed, 562 failed (559 distinct names), 7 ignored in 358 test binaries (run 4: 280 binaries, 2666 passed, 813 failed, 7 ignored).

### The hang (class d, "hang, ended by the job limit")

- Binary: the unit tests of `horch-platform` (`Running unittests src\lib.rs ... horch_platform`). It has no line `test result:`.
- The last test that started: `windows::channel::tests::client_that_never_reads_the_reply_does_not_block_the_next_client`. The log line `has been running for over 60 seconds` is at 10:14:20Z. No line follows until the cancel at 11:36:38Z (1 h 22 min).
- Cause file: `crates/horch-platform/src/windows/channel.rs:793-811`. The test opens a client that sends 1 line and never reads the reply, and then connects a second client with `far_deadline()`. The test expects that the server ends the first connection after `DRAIN_LIMIT`. The hang has no deadline that ends the test: probable cause, a read or a connect that the Windows channel never ends. Not proved. T-38 (done) made the reply writes bounded on Windows; its test for this case never ends in CI.
- The 4 tests of `fleet18_messaging.rs` that hung in run 4 (`g60_missing_registration_still_queues_worker_mail`, `g60_optional_question_projection_error_does_not_abort_courier_startup`, `g62_courier_notifies_once_and_resumes_after_login`, `g62_login_screen_is_detected_before_typing`) pass now. FIX-39 (T-342) works for them, and the job limit worked.
- Binaries after `horch-platform` (27 of 385) have no result in this run.
- Class (e): `test-windows` stops now, but only after 120 min. The cause of the hang is a test, not the setup.

### Groups of the 562 failures

| Group | Count | Class | First error line | file:line of the cause |
|---|---|---|---|---|
| A verbatim path `\\?\C:\...` goes to `git`, or the `worktree_add` job never ends | 157 | d | `worktree \\?\C:\Users\runneradmin\...\r1-T1-after is not made yet: the worktree_add job 1 is running; run the command again` (76 tests); `git ["worktree", "add", "--quiet", "--detach", "\\\\?\\C:\\...\\gate-F-0001", ...]: fatal: could not create leading directories` (9); `git ["read-tree", ...]: fatal: Unable to create '\\\\?\\C:\\...\\scratch-index.lock': Invalid argument` (11) | `crates/horch-core/src/fleetd/task_execution/application/launch.rs:793`; `crates/horch-core/tests/feature_states.rs:83`; `stacks.rs:751`; `trial_merges.rs:283`. Files: `w3_finish_steps.rs` 38, `mod.rs` 21, `land.rs` 12, `skills_exposure.rs` 11, `runs.rs` 9, `fleet18_observability.rs` 7, `launch.rs` 6. T-355 landed (`f18eb76e`); the group is still red. The product code that passes the path is not checked. |
| `git` runs only in a sandbox, and Windows has none (FIX-37, open operator decision) | 105 | d | `no sandbox: sandbox is not supported on windows: Windows has no Seatbelt and no bubblewrap; fleetd runs git in a project only in the sandbox, so the job ran no git` | `crates/horch-core/src/fleetd/integration/application/commands.rs:407` (80 tests). Files: `bundles.rs` 15, `integration_git.rs` 13, `idea_direct.rs` 12, `worktree_jobs.rs` 10, `ref_move.rs` 9, `branch_abandon.rs` 6, `merged_with_ideas.rs` 6 |
| The launch refuses: `Claude Code has no sandbox on windows` (FIX-37, same decision) | 55 | d | ``called `Result::unwrap()` on an `Err` value: sonnet: refusing to launch without a sandbox: Claude Code has no sandbox on windows`` | `crates/horch-core/src/execution/lifecycle.rs:2132` (9), `harness/claude.rs:2899` (8), `p5_claude_sandbox.rs:119` (7), `intake_roster.rs:284` (4). Files: `launch.rs` 15, `p7_conformance.rs` 15, `intake_roster.rs` 5, `p5_claude_sandbox.rs` 4, `harness_defaults.rs` 3, `p7_claude_exec.rs` 3 |
| A job stays `Running` | 19 | d? | ``assertion `left == right` failed left: Running right: Passed`` | `crates/horch-core/tests/fetch_job.rs:450`, `push_job.rs:614`, `:578`, `worktree_jobs.rs:65` (`HOME is set`). Files: `worktree_jobs.rs` 8, `push_job.rs` 7, `fetch_job.rs` 3, `fleetd_jobs.rs` 1. Probable cause: the same as the 2 sandbox groups. |
| Access denied (os error 5) | 13 | d? | `PermissionDenied ... marketplace.lock: Access is denied. (os error 5)` | `crates/horch-marketplace/tests/marketplace.rs:321` (7), `crates/horch-core/src/skills/catalog.rs:1031` (2), `harness/trust.rs:1787`, `lock.rs`, `lockfile.rs`, `fleet18_messaging.rs` |
| Expected text with LF only, a golden file, or CRLF output | 10 | c | ``assertion `left == right` failed`` on goldens (`golden_prompts.rs` 3, `r_routing_assess.rs`, `r_routing_parsers.rs`, `r_runtime_skills_materialize.rs`, `telemetry_collect.rs`, `skills_cli.rs`, `context_launch.rs`) | e.g. `crates/horch-core/tests/golden_prompts.rs`, `r_routing_assess.rs` (the golden path is printed as `D:\a\multi-herdr\...\tests/golden/...`) |
| Codex phase skills need a private `CODEX_HOME` | 9 | c | `Codex phase skills require a private CODEX_HOME; use WSL on Windows, or explicitly set phase: null and skills: [] for the legacy shared-rules launcher` | `crates/horch-e2e/tests/messaging.rs:147` (6), `crates/horch-core/tests/execution_plan.rs:521` (3) |
| The owner SID of the idea file is not the ledger owner | 7 | d? | `Rejected { file: "idea.md", reason: "owner sid S-1-5-32-544 is not the ledger owner sid S-1-5-21-..." }` (the runner is an administrator) | `crates/horch-core/tests/idea_intake.rs:125` (3), `idea_waiting.rs` (2), `fleetd_handlers_idea.rs`, `fakes.rs` |
| The pipe is not connected (error 233) | 6 | d | ``called `Result::unwrap()` on an `Err` value: Os { code: 233, ... "No process is on the other end of the pipe." }`` | `crates/horch-core/tests/fleetd_channel.rs:157` |
| `timed out after 120 s waiting for the worktree of the task` and similar waits | 5 | d? | `timed out after 120 s waiting for ...` | `crates/horch-e2e/src/harness.rs:1409` (`g56_...`, 4 `g69_drop_*` tests). The product does not run `git` in a sandbox: probable cause, the same as the 2 sandbox groups. |
| The CRLF parser test of T-339 | 2 | d | ``called `Result::unwrap()` on an `Err` value: file does not start with a '---' frontmatter fence`` | `crates/horch-core/src/roster/parser.rs:138` (`a_crlf_file_parses_to_the_same_teammate_and_base_as_its_lf_form`); `crates/horch-core/src/roster/tests.rs:266` (`f7_bad_base_file_keeps_the_builtin_base`) |
| Other | 170 | d? | one line for each | by file: `taskledger/map.rs` 10 (`tickets\\MAP.md:3` expects `/`), `cmd/context.rs` 8 and `context_cli.rs` 6 (the table of `context`), `context_roster.rs` 7, `telemetry_collect.rs` 7 (`rec-b1: -0`), `cmd/compact.rs` 6, `context_prime.rs` 6, `e2e` harness `fleetd.rs:138` 6 (`signal_same`), `w3_ledger_cli_pins.rs` 5 (`.\\.horch\\ledger.sqlite3`), `fleetd_pass.rs` 5 (`Os { code: 3 }`), `fleet18_observability.rs` 4 (`%1 is not a valid Win32 application`), and about 70 tests in 40 files with 1 or 2 tests each. The log of the job is the source. |

Total of the groups: 157 + 105 + 55 + 19 + 13 + 10 + 9 + 7 + 6 + 5 + 2 + 170 = 558. Each failure block went to the first rule that matched its text. The log has 562 failed lines but 558 blocks `---- <test> stdout ----`, because 4 test names exist in 2 binaries and share 1 block. Use the counts as sizes.

How many of the 813 failures of run 4 are green now (by test name; a name that exists in 2 binaries counts once):

| Of the failed tests of run 4 | Count |
|---|---|
| green now | 316 |
| still red | 498 |
| not run now | 0 |
| red now, and `ok` in run 4 | 1 (`git_env_cannot_reach_another_repository`, `skills_exposure.rs`) |
| red now, and not in run 4 at all (the binaries ran for the first time) | 61 |

By group of run 4: the CRLF roster group (369) is green but for 2 tests; the `/tmp` group (37) and the `node` group (8) are green; the 4 hang tests of `fleet18_messaging.rs` are green. The verbatim path group (163 in run 4, 157 now) and the sandbox groups (83 in run 4, 160 now, because more binaries ran) stay. FIX-37 (the sandbox for git on Windows) is an open operator decision: the 2 sandbox groups hold 160 failures, plus about 24 more of the groups "A job stays `Running`" and "timed out" that have the same probable cause. I did not fix them.

## Job `docs`

Failure. 6 errors (run 4: 1 error in `ingest.rs:16`, which is fixed by T-331).

| Error | file:line |
|---|---|
| ``public documentation for `GroupLeftHold` links to private item `GROUP_LEFT` `` | `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:419` |
| ``public documentation for `group_left_holds` links to private item `GROUP_LEFT` `` | `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:447` |
| ``public documentation for `git_writer_busy` links to private item `note_group_left` `` | `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:852` |
| ``public documentation for `kill_job_group_on_stop` links to private item `GIT_GROUP` `` | `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:762` |
| ``unclosed HTML tag `pid` `` | `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:437` (the text `"pid <pid> (start time <start>)"`) |
| ``unclosed HTML tag `start` `` | `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:437` |

Cause: commit `7a8e2a27` "fleetd-finish 208: each stop of a job ends its sandbox on Linux ... (T-319)". Class d (doc comments of the product). Ticket 256 (T-398, landing) owns `jobs.rs` and `worktree_custody/adapters/git.rs` but its Check has no `cargo doc`.

## Job `release-lint` (`cargo clippy --workspace --bins --release --locked -- -D warnings`)

Success.

## Difference to run 4

CI4 = run 44861461538 (head `2c6ef0b4`). CI5 = this run (head `05ff4081`).

### Per job

| Job | CI4 | CI5 | Change |
|---|---|---|---|
| `test (macos-15)` | 5186 passed, 23 failed | 5270 passed, 9 failed | better: 19 of 23 green, 5 new red |
| `test (ubuntu-latest)` | 5157 passed, 52 failed | 5272 passed, 8 failed | better: 51 of 52 green, 7 new red |
| `docs` | failure, 1 error | failure, 6 errors | the CI4 error is fixed; **new red**: 6 errors of T-319 |
| `release-lint` | success | success | no change |
| `build-windows` | success | success | no change |
| `test-windows` | cancelled after 3 h; 2666 passed, 813 failed | cancelled by the limit of 120 min; 3133 passed, 562 failed; 358 binaries | the limit works; 316 of the 813 are green; 1 new hang |

### Each FIX-nn of run 4

| FIX | Task | State now |
|---|---|---|
| FIX-26 `docs` `ingest.rs:16` | T-331 | **green**. The `docs` job has 6 other errors (FIX-41 below). |
| FIX-27 Linux PATH of the pass helpers (10 tests) | T-332 | **green** |
| FIX-28 "2 active runs" (7 tests) | T-333 | **green** |
| FIX-29 `skills_exposure.rs` (4) | T-334 | **green** |
| FIX-30 steward `triage_*`, `g50_...`, `rr10_...` | T-335 | **green** (the 3 class (a) steward tests are green too) |
| FIX-31 polite-stop probe file | T-333 | **green** |
| FIX-32 5 `taskledger.rs` tests | T-336 (cancelled), landed by T-364 | **green**, but `the_whole_flow_on_the_fake_herdr` is still red on Linux with a new message (class a: the list reason differs) |
| FIX-33 Linux sandbox fixtures (4) | T-337 | **green** |
| FIX-34 Linux worktree waits (8) | T-338 | **green** |
| FIX-35 Windows CRLF roster (369) | T-339 | **green**, except 2 tests (`parser.rs:138`, `tests.rs:266`) |
| FIX-36 Windows verbatim path (163) | T-340 (cancelled), T-348, T-355 | **still red**: 157 tests |
| FIX-37 Windows sandbox for git (83) | open operator decision | **still red**: 105 + 55 tests (more binaries ran) |
| FIX-38 Windows `/tmp`, `node`, CRLF text (51) | T-341 | **green** for `/tmp` (37) and `node` (8); 10 tests with LF-only text or goldens stay |
| FIX-39 Windows hang and the job limit | T-342 | **green** for the 4 hang tests and the limit; **new hang** in `horch-platform` |
| FIX-40 Windows other (154) | none | partly green; see the groups above |
| The 4 `restore_*` tests of `p3_worktrees_cli.rs` (macOS) | T-290 (done) | **still red** with the same message |

### New red tests (red now, green or not red in run 4): regressions of the fix wave first

| Test | OS | Cause | Commit or task |
|---|---|---|---|
| `the_hermetic_story`, `the_hermetic_story_with_an_ollama_that_starts_in_7_s` | macOS, Linux | the test waits for the old text `launch_failed: REFUSED: opus cannot start.`; the row holds `its spawn failed: REFUSED: ...` | `d8a08e35` (T-254, FIX-25) added the prefix. Ticket 255 (T-397, running) covers it. |
| `arc_14_provenance_on_spawn_substitute_resume` | macOS, Linux | the same text change | ticket 255 (T-397) names "maybe 1 routing test": this is that test |
| `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing` | macOS, Linux | `end_reason` starts with `its spawn failed: no brief at`, the test expects `no brief at ` | `d8a08e35`. **No ticket** (T-397 owns only `scenario.rs` and `routing_provenance.rs`). |
| `g56_later_spawn_receives_recorded_fact` | macOS, Linux | no `claude` call with the project fact in 120 s; not proved | **no ticket** |
| `arc_26_e2e_lifecycle_matrix_claude` | Linux | the attempt is not live in 120 s; not proved | **no ticket** (tickets 140 and 187 are old) |
| `rr4_file_request_and_job_events_wake_pass_before_period` | Linux | the ledger commit does not wake a pass in 15 s | **no ticket** (1 failure in 1 run; macOS green) |
| `docs`: 6 errors in `jobs.rs` and `git.rs` | Linux | doc links to private items, `<pid>` in a doc comment | `7a8e2a27` (T-319); **no ticket** |
| Windows `windows::channel::tests::client_that_never_reads_the_reply_does_not_block_the_next_client` | Windows | hang; the binary ran for the first time | **no ticket** |
| Windows `git_env_cannot_reach_another_repository` | Windows | `ok` in run 4, red now; not read | **no ticket** |

### Tasks that cover each failure that is still red

State of the run board at 11:39Z (`horch ledger status --run fleetd-finish`): 402 tasks, 340 done, 5 running, 3 landing, 13 queued, 41 cancelled.

| Failure | Task |
|---|---|
| 2 hermetic story tests, `arc_14_...` | T-397 (ticket 255, running) |
| `the_whole_flow_on_the_fake_herdr` (Linux) | T-400 (ticket 258, queued, blocked by T-289 landing) for the timeout; T-94 (queued) for the list entry. T-94 still names T-13 (cancelled) as its dependency. |
| The `p0_liveness` regression (not red in this run) | T-399 (ticket 257, running). Listed here only because it is the run of a later tip. |
| The 6 `docs` errors | no ticket. T-398 (ticket 256, landing) edits the same 2 files. |
| `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing` | no ticket |
| The 4 `restore_*` tests (macOS) | no ticket. T-290 is done and did not fix them. |
| `g56_...`, `arc_26_...`, `rr4_...` | no ticket |
| Windows: the hang of `horch-platform`, the 2 CRLF tests, `git_env_cannot_reach_another_repository`, and the groups with no FIX task | no ticket |
| Windows: verbatim path (157) | T-355 landed and the group is red: no open ticket |
| Windows: sandbox for git (160) | the operator decision FIX-37 (no task) |

### `no ticket` rows of run 4 and their state now

| Row of run 4 | State now |
|---|---|
| FIX-26 to FIX-34 | **fixed** (T-331 to T-338, T-364), except `the_whole_flow_on_the_fake_herdr` |
| The 4 restore tests (T-290) | **still red** |
| FIX-35, FIX-38, FIX-39 | **fixed**, with the exceptions in the table above |
| FIX-36 | **still red** |
| FIX-37 | **open operator decision**: 160 failures |
| FIX-40 | **partly fixed**; 170 tests in the group "Other" |
| `p7_conformance_binary.rs:465`, `p5_operator_boundary.rs:802` (Linux) | **green** |

## Fix units

Only failures with no ticket. Each unit is for 1 worker. The commands run through `scripts/dev-sandbox.sh` (rule 2 of the run). I did not run any of them: this task reads the CI only. A unit says "no macOS repro" when the failure is Linux-only or Windows-only.

| Unit | Fixes | Files it touches | Command that reproduces 1 failure |
|---|---|---|---|
| FIX-41: the 6 rustdoc errors | the `docs` job | `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:419`, `:437`, `:447`, `:852`, `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:762` (name each private item in code font, or write `<pid>` in code font). Start after T-398 lands: it edits the same 2 files. | `RUSTDOCFLAGS='-D warnings' scripts/dev-sandbox.sh cargo doc --workspace --no-deps --all-features --locked` (fails on macOS the same way: it is the line of the `docs` job) |
| FIX-42: the end reason of a failed spawn in `startup_error.rs` | `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing` (macOS, Linux) | `crates/horch-e2e/tests/startup_error.rs:224` (the whole designed text `its spawn failed: no brief at ...`, as an exact match; the same fix as T-397 for the 2 other files) | `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test startup_error spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing` |
| FIX-43: `g56_later_spawn_receives_recorded_fact` | 1 test (macOS, Linux) | `crates/horch-e2e/tests/fleet18_brief_worktree.rs:380` (give the wait a context: `launch.log` and the calls of `claude`; then find the cause) | `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test fleet18_brief_worktree g56_later_spawn_receives_recorded_fact` |
| FIX-44: the 4 `restore_*` tests | 4 tests (macOS) | `crates/horch/tests/p3_worktrees_cli.rs:153` and the rescue path of fleetd (the test waits 120 s for a rescue; T-290 changed only `worktree_list_period_s`). Read the diagnostics of the world before a change. | `scripts/dev-sandbox.sh cargo test -p horch --locked --test p3_worktrees_cli restore_runs_no_hook_of_the_project` |
| FIX-45: 2 Linux e2e tests | `arc_26_e2e_lifecycle_matrix_claude`, `rr4_file_request_and_job_events_wake_pass_before_period` (cause not proved; run each 3 times first, rule 6) | `crates/horch-e2e/tests/lifecycle.rs:209`, `crates/horch-e2e/tests/p1_pass.rs:106` | no macOS repro (both are green on macOS). On Linux: `cargo test -p horch-e2e --locked --test lifecycle arc_26_e2e_lifecycle_matrix_claude` and `cargo test -p horch-e2e --locked --test p1_pass rr4_file_request_and_job_events_wake_pass_before_period` |
| FIX-46: Windows, the hang of the channel unit test | 1 test that never ends, and the next 27 binaries | `crates/horch-platform/src/windows/channel.rs:793-811` (the test) and the read or connect path of the Windows channel that the test uses (a deadline that ends the first client) | no macOS repro (Windows-only). `scripts/dev-sandbox.sh cargo test -p horch-platform --locked --lib windows::channel` builds only on Windows. |
| FIX-47: Windows, 2 CRLF tests | `a_crlf_file_parses_to_the_same_teammate_and_base_as_its_lf_form`, `f7_bad_base_file_keeps_the_builtin_base` | `crates/horch-core/src/roster/parser.rs:138`, `crates/horch-core/src/roster/tests.rs:266` (the test builds its CRLF text; read how the runner changes it) | no macOS repro. A check on macOS: `scripts/dev-sandbox.sh cargo test -p horch-core --lib roster::parser` |
| FIX-48: Windows, the verbatim path group that T-355 left | 157 tests | `crates/horch-core/src/fleetd/task_execution/application/launch.rs:793`, `crates/horch-core/tests/feature_states.rs:83`, `stacks.rs:751`, `trial_merges.rs:283`; first read whether the `worktree_add` job fails by the path or by the missing sandbox (FIX-37) | no macOS repro (Windows-only) |
| FIX-49: Windows, the groups with test-side causes | LF-only text and goldens (10), Codex `CODEX_HOME` (9: gate with `#[cfg(unix)]` or set `phase: null`), owner SID (7), pipe 233 (6), access denied (13), `taskledger/map.rs` (10), `context` table (14), the 6 `fleetd.rs:138` tests | `crates/horch-core/tests/golden_prompts.rs`, `crates/horch-e2e/tests/messaging.rs:147`, `crates/horch-core/tests/idea_intake.rs:125`, `fleetd_channel.rs:157`, `crates/horch-marketplace/tests/marketplace.rs:321`, `crates/horch-core/src/taskledger/map.rs`, `crates/horch/src/cmd/context.rs:1206`, `crates/horch-e2e/src/fleetd.rs:138`, `skills_exposure.rs` (`git_env_cannot_reach_another_repository`) | no macOS repro (Windows-only) |

Not a unit: FIX-37 (the 160 failures with no sandbox for git or for the launch on Windows). It stays an operator decision: a sandbox for Windows, or `#[cfg(unix)]` on these tests.

Order: FIX-42 first (small, 1 line of text). FIX-41 after T-398 lands. FIX-43 to FIX-45 need a diagnosis first. FIX-46 before the next run of the Windows job, because the hang hides the last 27 binaries. After T-397, T-400, T-94, FIX-41 to FIX-45: run the CI 1 more time.
