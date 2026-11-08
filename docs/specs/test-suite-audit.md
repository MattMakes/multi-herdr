# Test suite audit (T-38, 2026-11-07)

Scope: every Rust test in `crates/*` (integration tests and `#[cfg(test)]`
modules) and the Python self-tests in `skills/hark/engine/tests/`,
`skills/one-page-design/scripts/tests/`, `scripts/skill-intake/tests/` and
`scripts/godot/tests/`. Base: `fleet/solo-w68-hark` at `d6e252ef`. Host:
macOS aarch64, 18 CPUs, 1-minute load 7 to 18 during the runs (the host ran
other fleets).

This task changed no test or code. Each unit in [Units](#units) is a fix
for 1 worker. The units own disjoint files.

## Baseline

### Rust: `cargo test --workspace --no-fail-fast -- --test-threads=18`

Per-test times come from libtest JSON (`RUSTC_BOOTSTRAP=1 ... -Z
unstable-options --format json --report-time`).

| Pass | Wall | Load at start | Passed | Failed | Ignored |
|---|---|---|---|---|---|
| 1 (cold: first exec of every new test binary) | 406 s | 11.7 | 2802 | 2 | 11 |
| 2 | 327 s | 8.9 | 2802 | 2 | 11 |
| 3 | 285 s | 7.7 | 2802 | 2 | 11 |

- Build of all test binaries (`cargo test --workspace --no-run`, warm
  dependencies): 44 s.
- 124 test binaries, 2815 tests. `cargo test` runs the binaries one after
  another, so the wall time is close to the sum of each binary's slowest
  test, not the total CPU time. Sum of all per-test times: 1371 s (76 s per
  thread at 18 threads).
- 295 tests take over 1 s. 63 tests take over 5 s.
- The 2 failures are deterministic on this branch, not flaky:
  `skills_catalog::skills_bundled_text_only` (`hark/engine/hark/__init__.py`
  is not `.md`/`.txt`) and `skills_catalog::skills_bundled_size_budget`
  (`hark: 1543890 bytes`). The hark skill work causes both. CI shows the same
  2 failures on this branch since 2026-11-07 22:30. This audit does not own
  them (see [Open questions](#open-questions), Q4).

### Python

| Suite | Runner | Tests | Result | Wall | Runs in |
|---|---|---|---|---|---|
| `skills/hark/engine/tests` | pytest 8 | 827 | 668 pass, 88 fail, 71 skip | 105 s | nowhere |
| `scripts/skill-intake/tests` | unittest | 401 | OK | 138 s | `scripts/phase-gate.sh` |
| `scripts/godot/tests` | unittest | 71 | OK, 1 skip | 18 s | `scripts/phase-gate.sh` |
| `skills/one-page-design/scripts/tests` | unittest | 98 | 1 error | 0.4 s | nowhere |

- The 88 hark failures are deterministic: `hark.forge.engine.fx` has no
  `saturate` (52) and `hark.forge.engine.dsp` has no `modal` (26), plus the
  imports that follow from them. The hark engine work is in progress on this
  branch.
- The one-page-design error is
  `test_starter_page.Starter.test_new_size_prints_at_that_size`: it finds
  `/opt/homebrew/bin/chromium`, and that binary exits 126 here. The test
  skips only when no Chrome is found, not when Chrome cannot run.
- No CI job and no gate step runs the hark or one-page-design suites. A
  failure in them is seen only when a worker runs them by hand.

### CI history (`gh run list --limit 50`)

48 `ci` runs: 19 success, 25 failure, 4 in progress. The failed jobs name 7
tests:

| Test | Failed jobs | Kind |
|---|---|---|
| `baseline_oracles::oracle_skills_match`, `oracle_launch_matches`, `arc_09_argv_matches_baseline` | 9 runs each (`fleet/video-lessons`) | deterministic: the frozen oracles broke when the launch prompt text changed (see T6) |
| `w3_ledger_cli_pins::a_worker_claims_makes_a_worktree_and_finishes` | 7 runs, both OS | deterministic on `fleet/video-lessons`; passes on this base |
| `skills_catalog::skills_bundled_text_only`, `skills_bundled_size_budget` | 5 and 3 runs | deterministic (hark files) |
| `execution_store::store_dirlock_excludes_concurrent_writers` | 4 runs, both OS, 3 branches | flaky (F1) |
| `procid::tests::a_new_child_started_after_this_process` | 1 run, ubuntu | flaky (F2) |

## Slow

The slowest 30 tests, mean of the 3 full passes (min to max in brackets).
"Binary" is the test target. A test that is the slowest in its binary sets
that binary's wall time, so the "Saves" column counts only the drop of the
binary's slowest test.

| # | s | Test | Binary | Cause | Fix (unit) |
|---|---|---|---|---|---|
| S1 | 30.8 (30.7-30.8) | `harness::codex::tests::w3_a_command_past_its_deadline_is_killed_and_named` | horch-core lib | `fake_codex` warms the new script by running it, and the body is `sleep 30` (`codex.rs:1631`). The deadline part takes 0.3 s. | Warm with a guard argument, as `quota_probe.rs:875` does (U3) |
| S2 | 21.5 (21.4-21.9) | `bal_08_fleet_auto_choice` | e2e | 4 sequential `horch fleet` runs. Each waits the full 5 s fleet delivery probe, because the fake herdr never acknowledges it (`delivery_probe.rs:99`). | Harness default `HORCH_FAULT=delivery-probe-ms:100`; split into 4 tests (U1) |
| S3 | 21.1 | `ledger_done_finishes_in_time_against_a_stuck_orchestrator` | done_budget | Waits the real `DONE_PUSH_BUDGET` of 20 s (`delivery.rs:242`) | Fault point for the budget (U2) |
| S4 | 20.6 | `a_pass_with_an_unknown_orchestrator_ends_within_the_push_budget` | fleet18_messaging | Same 20 s budget | U2 |
| S5 | 20.6 | `done_returns_in_time_when_herdr_hangs_on_pane_get` | done_budget | Same 20 s budget | U2 |
| S6 | 20.5 | `done_finishes_in_time_against_a_stuck_orchestrator` | done_budget | Same 20 s budget | U2 |
| S7 | 16.2 (15.6-17.3) | `telemetry::collect::tests::nfr_02_indexed_fold_matches_the_event_sums` | horch-core lib | 200 property cases in a debug build (13.3 s alone). The sibling property test uses 40. | 40 cases (U3) |
| S8 | 15.1 | `the_hermetic_story_with_an_ollama_that_starts_in_7_s` | scenario | 7 s fake ollama delay plus 5 s fleet delivery probe | U1 removes the 5 s |
| S9 | 11.6 | `arc_34_no_update_and_horch_no_update_run_no_updater` | update | 2 fleets, 5 s delivery probe each | U1 |
| S10 | 11.5 | `arc_33_a_second_fleet_within_24_h_skips_the_updated_clis` | update | 2 fleets, 5 s probe each | U1 |
| S11 | 10.8 | `arc_33_a_hung_updater_is_left_running_at_the_timeout` | update | Updater timeout plus 5 s probe | U1 |
| S12 | 10.5 | `doctor_warns_when_the_codex_reviewer_has_no_codex` | doctor_extras_reviewer | 2 `horch doctor` runs, 5 s doctor delivery probe each (`delivery_probe.rs:42`; 1.1 s CPU in 10.5 s wall) | U1 |
| S13 | 10.3 | `a_detached_finish_returns_at_once_and_mails_the_result` | taskledger | Full git finish flow with fakes | none: the binary has 68 tests, and 12 more of them take 6 to 9 s |
| S14 | 9.3 | `routing::quota_probe::tests::horch_local_probe_timeout_ms_sets_the_local_deadline` | horch-core lib | Real `sleep 7` fake ollama | Sleep 0.5 s (U3) |
| S15 | 9.3 | `the_whole_flow_on_the_fake_herdr` | taskledger | Full flow | none |
| S16 | 9.1 | `bal_06_route_matches_gate` | e2e | Fleet runs with the 5 s probe | U1 |
| S17 | 8.8 | `spc_03_a_slow_collector_start_loses_no_event` | telemetry_start_handshake | Intended slow collector start | none |
| S18 | 8.5 | `a_sigterm_during_a_reap_lets_the_reap_finish` | watchdog_liveness | Intended reap timing | none |
| S19 | 8.3 | `a_codex_workers_check_cannot_write_outside_its_worktree` | taskledger | Full flow | none |
| S20 | 8.3 | `cmd::doctor::tests::xcode_export_names_a_missing_xcrun_and_an_unreadable_version` | horch bin | 1.0 s alone. Many tests write new fake scripts at once, and macOS scans each new executable on its first exec. | See S-note 1 |
| S21 | 7.9 | `worker_reads_the_skill_store_its_spawner_read` | skills_exposure | Fleet with the 5 s probe | U1 |
| S22 | 7.9 | `cmd::doctor::tests::csharp_project_without_the_dotnet_engine_is_reported` | horch bin | First-exec contention | S-note 1 |
| S23 | 7.8 | `cmd::doctor::tests::xcode_export_on_27_is_verified` | horch bin | 0.97 s alone; first-exec contention | S-note 1 |
| S24 | 7.6 | `g53_ledger_handoff_compacts_resumes_and_records_progress_once` | fleet18_observability | Compaction flow | none |
| S25 | 7.4 | `an_outer_git_dir_reaches_no_child_of_finish` | taskledger | Full flow | none |
| S26 | 7.3 | `a_landed_worktree_with_no_new_commit_passes_land_without_a_push` | taskledger | Full flow | none |
| S27 | 7.0 | `routing::quota_probe::tests::a_slow_ollama_times_out_at_the_5_s_production_deadline` | horch-core lib | Real 5 s production deadline against `sleep 7` | Pass a 300 ms deadline; keep the 5 s value check (U3) |
| S28 | 7.0 | `cmd::compact::tests::a_slow_child_within_the_default_start_timeout_is_scheduled` | horch bin | Intended start timeout | none |
| S29 | 6.7 | `a_rust_only_change_is_not_sent_to_the_complexity_analyser` | taskledger | Full flow | none |
| S30 | 6.6 | `cmd::doctor::tests::godot_below_the_floor_or_without_a_version_is_reported` | horch bin | First-exec contention | S-note 1 |

Other tests that pay the 5 s delivery probe (U1 fixes them too):
`tel_02_fleet_records_the_orchestrators_plugin_skills` (5.8 s),
`tel_02_fleet_writes_orchestrator_record` (5.7 s),
`spc_06_fleet_survives_ensure_failure` (5.5 s),
`t11_followup_orchestrator_registers_under_a_custom_state_dir` (5.5 s),
both tests in `doctor_extras_watchdog` (5.3 s each), and 5 more `arc_32`,
`arc_33` and `arc_36` tests in `update` at 7.0 s each.

Python:

| # | s | Test | Cause | Fix (unit) |
|---|---|---|---|---|
| S31 | 138 (suite) | `scripts/skill-intake/tests` | unittest runs 401 tests in 1 process, 1 at a time | Run the test modules in parallel processes in `phase-gate.sh` (U7) |
| S32 | 15.9 | `test_argv.ArgvTest.test_t14_no_command_runs_a_quarantine_file` | The whole intake pipeline, 10 subprocess calls | none (it is the end-to-end check) |
| S33 | 5.1 | `test_roles.ModeTest.test_a_horch_that_hangs_past_5_s_is_plain` | Real 5 s `VERSION_TIMEOUT` (`skills/skill-intake/scripts/intakelib/task.py:98`) | An env knob for that timeout (U7) |
| S34 | 13.8 | `tests/timeline/test_sheet.py::test_s7_long_film_text_fits` | Renders a long film sheet | none until hark runs in a gate (Q2) |

S-note 1 (not a unit): macOS scans each new executable on its first exec.
When many tests in 1 binary write new fake scripts at the same time, the
scans queue, and a 1 s test takes 8 to 10 s (S20, S22, S23, S30, and
`workspace::herdr::tests::pane_focus_walk_*` at 10 s in the suite and 0.6 s
alone). The fix is 1 shared, warmed fake per test module that reads its
case from an environment variable. That is a wide change for about 5 s per
binary. Do it only after U1 to U3.

### Expected saving

| Binary | Now | After | Unit |
|---|---|---|---|
| horch-core lib | 31.5 s | about 10 s (pane_focus_walk then sets it) | U3 |
| e2e | 21.5 s | about 5 s | U1 |
| done_budget | 21.1 s | about 3 s | U2 |
| fleet18_messaging | 20.6 s | about 4 s | U2 |
| update | 11.6 s | about 2.5 s | U1 |
| doctor_extras_reviewer | 10.5 s | about 0.5 s | U1 |
| scenario | 15.1 s | about 10 s | U1 |
| doctor_extras_watchdog | 5.4 s | about 0.5 s | U1 |
| skills_exposure, messaging (e2e) | 7.9 s, 6.0 s | about 3 s each | U1 |

Total: about 105 s off the 285 s warm wall time, about 37%.

A larger change is a parallel test runner. `cargo nextest` runs tests from
all binaries at once, so the wall time falls toward the longest single test
(31 s now, about 10 s after U1 to U3) or the CPU bound (about 76 s at 18
threads). It needs a new tool in CI, the gate and `just verify`. See Q1.

## Flaky

Method: 3 full passes, then 10 runs of each of 18 binaries (the 10 slowest
plus the socket-, process- and lock-heavy ones: horch-core lib, horch bin,
e2e, done_budget, fleet18_messaging, taskledger, scenario, update,
doctor_extras_reviewer, telemetry_start_handshake, execution_store,
channel, mail, both messaging binaries, watchdog_liveness, fakes, procid,
fleet18_observability) at `--test-threads=12`, then targeted stress runs.
See the repeat-loop result below the table.

| # | Test | Failure rate | Cause | Fix (unit) |
|---|---|---|---|---|
| F1 | `crates/horch-core/tests/execution_store.rs:289` `store_dirlock_excludes_concurrent_writers` | 96 of 200 runs alone; 0 of 3 in the full suite; 4 CI jobs in 48 runs | Product race. 2 threads open a new ledger file at once. `Store::open` runs `PRAGMA journal_mode = WAL` (`taskledger/store.rs:149`), and SQLite returns `SQLITE_BUSY` for that change at once, without the busy handler. The error is "database is locked" from `Store::open`. A fleet that starts 2 horch processes on a new state dir can hit the same race. | Retry the journal-mode change on `SQLITE_BUSY` until `BUSY_TIMEOUT` (U4) |
| F2 | `crates/horch-core/src/procid.rs:500` `a_new_child_started_after_this_process` | 0 of 100 runs here (macOS); 1 CI job in 48 (ubuntu) | Linux start times count from a boot time in whole seconds, so the child's start time can read up to 1 s plus 1 clock tick early. The test allows exactly 1 s. CI: `theirs` 1794088906.330, `before` 1794088907.333, 3 ms past the allowance. | Allow 2 s on Linux (U3) |
| F3 | `skills/one-page-design/scripts/tests/test_starter_page.py:100` `test_new_size_prints_at_that_size` | 1 of 1 here | Host-dependent: Chrome is found but cannot run (exit 126). The test skips only when no Chrome is found. | Probe `chrome --version`; skip when it fails (U8) |
| F4 | `crates/horch-e2e/tests/fleet18_observability.rs:317` `g52_concurrent_workers_get_their_own_session_and_transcript` | 1 of 10 loop runs (round 9, load 8.9); 0 of 3 in the full suite | Same race as F1, through 2 real horch processes: `horch: opening the session records in .../state/ledger.sqlite3: database is locked` (line 361). This proves the F1 race reaches production commands. | U4 fixes it; no test change |

Repeat loop: 10 rounds of the 18 binaries, 190 binary runs (both messaging
binaries count, 19 targets per round), 1-minute load 4.5 to 9.2. 189 runs
passed. 1 run failed: F4 in round 9. F1 did not fail in the loop, because
the 2 writers rarely collide while other binaries load the host; it fails
96 of 200 times alone. The 3 full passes had 0 flaky failures.

Process leak seen during the audit: 2 `horch mail courier --workspace w1`
processes from `.worktrees/T15-.../target/debug/horch` ran for 6.5 h with
parent pid 1. Their `HORCH_STATE_DIR` is an e2e harness dir
(`~/.local/state/horch/tmp/opus-28/h172e9-3/state`) and they hold
`courier.lock` there. A test binary that is killed before its harness
teardown runs leaves the courier, and the courier stays while its mail is
pending (`trf_04_courier_stays_while_mail_is_pending_and_the_pane_is_gone`).
See Q3.

## Tautological

Each flagged test fails a definition in the ticket. The other tests read
well: scans carry planted-pattern self-tests and minimum-count guards,
golden files name their origin, and property tests use fixed seeds. The
mechanical pre-filters that found the candidates are listed after the
table.

| # | File:line | Test | Reason | Verdict |
|---|---|---|---|---|
| T1 | `skills/hark/engine/tests/recipes_ui/test_ui_keys.py:125` | `test_typing_key_rng_formula` | Computes the expected value with the same formula as `typing.key_rng` (`key * 7919 + part * 104729 + 31`, `typing.py:52`). | REWRITE. Oracle: (a) `key_rng(7)` twice gives equal first 4 values; (b) `key_rng(7)` and `key_rng(8)` differ; (c) the first `uniform()` of `key_rng(7)` equals a literal recorded once and reviewed, which pins seed stability across versions. Mutation: return `default_rng(0)` for every key; (b) must fail. |
| T2 | `crates/horch-core/tests/telemetry_collect.rs:489` | `nfr_02_a_moving_window_drops_old_events_without_new_input` | The expected 5 h rollup is `store::rollup(&events, window_start("5h", later), ...)`, the same functions `tick` uses. A wrong `window_start` moves both sides. | REWRITE. Oracle: the token total of the fixture events inside `[later - 5 h, later]`, counted by hand from the A2 fixture and written as a literal. Mutation: make `window_start` ignore its `now` argument; the rewrite must fail. |
| T3 | `crates/horch-core/src/taskledger/finish/land.rs:1500` | `trf_06_the_land_lock_is_stale_after_the_check_timeout_plus_10_min` | Its 2nd assertion expects `MAX_CHECK_TIMEOUT + min(10)`, the same formula as the code. The 1st assertion (`min(120)` gives `min(130)`) is good. | REWRITE the 2nd assertion to `min(24 * 60 + 10)` (spec value). Mutation: set `MAX_CHECK_TIMEOUT` to 12 h; the rewrite must fail. |
| T4 | `skills/hark/engine/tests/fit_analysis/test_fit_acceptance.py:51,56,61,66` | `test_a_1_validation_clap`, `test_a_2_validation_mel_gap`, `test_a_3_validation_describe_tag`, `test_a_4_replication_benchmark` | Body is `pass` under an unconditional `@pytest.mark.skip`. No assertion. | REMOVE. Keep the "needs-data" note in the hark spec, not as tests. |
| T5 | `skills/hark/engine/tests/recipes_ui/test_ui_contract.py:179` | `test_at_3_to_16_belong_to_impacts` (14 parameter cases) | Every case calls `pytest.skip` first. No assertion. It records ownership only. | REMOVE. `tests/recipes_impacts` holds the real tests. |
| T6 | `crates/horch-core/tests/baseline_oracles.rs:459,475,554,650,674` and `crates/horch/tests/baseline_cli.rs` | `oracle_launch_matches`, `arc_09_argv_matches_baseline`, `oracle_routing_matches`, `oracle_ledgers_match`, `oracle_skills_match`, and the 2 `baseline_cli` tests | Snapshots written by the code with `HORCH_BLESS=1` (208 files, 3.7 MB in `tests/oracles/`). Nobody reviews 3.7 MB, so a bug present at capture is the expected value. They are change detectors: CI failed 9 runs on `fleet/video-lessons` on these 3 tests when the launch prompt text changed (the skill-phase line in `launch/orchestrator.json`). | REWRITE, low priority (Q5). Oracle: per behaviour, a small reviewed table derived from the spec (launch argv per harness from `docs/specs/architecture.md`, routing verdicts per fixture). Mutation: drop 1 flag from the Claude launch argv; the table test must fail and name the flag. |
| T7 | `crates/horch-core/tests/r_routing_assess.rs:73` | `assess_pool_matches_the_golden_file` | Compares with a 3888-line golden that "was written by the code before the #153 refactor". The refactor landed. | REWRITE, low priority (Q5). Oracle: 1 row per quota rule in the spec (exhausted at `>= 0.98`, the headroom bands, unknown readings), with the hand-checked state and reason. Mutation: change `>= 0.98` to `> 0.98` in `assess_pool`; the table must fail. Then delete `tests/golden/r_routing_assess.txt`. `r_routing_parsers.txt` (31 lines) is small enough to review once and keep. |

Pre-filters run over all 2822 Rust tests and 1171 Python test functions:

- tests with no assertion and no assert-like helper: 26 Rust, 11 Python.
  Each was read. 22 Rust tests delegate to a helper that asserts. 4 assert
  `Ok` with `unwrap`, and `Ok` is the behaviour under test. The Python hits
  are T4, T5 and helper calls that assert inside.
- `assert_eq!` with the same non-trivial function on both sides: 22 Rust,
  3 Python. Each was read. All but T1 are metamorphic or old-versus-new
  checks with an independent side.
- an expected value built by a production call that the test also calls:
  12 Rust. T2 is the only one where both sides share the code under test.
- formulas on production constants inside assertions: 30 Rust. T3 is the
  only one that rebuilds the code's formula.
- constant-against-literal assertions: 8 Rust, 26 Python. These pin values
  from a spec table (escape codes, release binaries, parameter counts), so
  they are not flagged.

Prose pins are not flagged as tautological. `golden_prompts.rs` (2160
lines), `context_roster.rs` and `test_skill_docs.py` (29 tests) assert
briefing and skill text. They fail when a rule is removed, so they guard
behaviour, but each prose edit means copying the new text into the test.
See Q6.

## Duplicates

| # | Tests | Behaviour | Verdict |
|---|---|---|---|
| D1 | `crates/horch-core/tests/nfr.rs:68` `nfr_03_repo_roster_check_passes`; `crates/horch-core/src/routing/decision.rs:735` `bal_02_repo_roster_passes`; `crates/horch-core/tests/context_roster.rs:103` `ctx_08_builtin_roster_passes_the_floor`; `crates/horch-core/tests/intake_roster.rs:249` `t26_the_roster_check_passes_for_the_intake_teammates`; `crates/horch-core/tests/harness_defaults.rs:594` `hdf_03_builtin_roster_has_no_repeats` | The shipped roster passes `Roster::check()`. `nfr_03` asserts the whole `check()` is empty. The other 4 assert that a filtered part of the same `check()` is empty (`fallback_problems`, "headroom", the intake names, "repeats"). `build.rs` bakes the same `teammates/` dir into `Roster::builtin()`. | Keep `nfr_03`. REMOVE the other 4. Update the test lists in `docs/specs/telemetry.md:803` (BAL-02), `docs/specs/context-policy.md:776` (CTX-08) and `:803` (HDF-03). Each ID keeps another test, so `scripts/check-req-coverage.sh` still passes. |

Checked and not duplicates: `git_env_cannot_reach_another_repository` in 4
crates (each guards its own crate's git calls), `trf_43_*` in
`horch/src/cmd/spawn.rs` and `horch-e2e/tests/skill_intake_spawn.rs` (unit
error choice versus end-to-end text), and 38 near-identical test pairs that
differ only in their input literals (parameter cases).

## Units

Each unit lists its files. No file is in 2 units. Each unit is 1 worker
session. Verify each unit by its own timing or loop command. A unit that
changes product code needs a Claude worker for the gate.

### U1: e2e harness skips the 5 s delivery probe by default

- Files: `crates/horch-e2e/src/harness.rs`, `crates/horch-e2e/tests/e2e.rs`.
- Speed: in `Harness::new`, set `HORCH_FAULT=delivery-probe-ms:100`. Every
  test of the probe itself already sets its own `HORCH_FAULT`
  (`fleet18_delivery_probe.rs`, `restored_pane.rs:202`), and `set`
  replaces the default. `scenario.rs:174` sets and unsets `HORCH_FAULT`;
  check that its story still passes. Split `bal_08_fleet_auto_choice` into
  4 tests, 1 per fixture, through a shared helper.
- Expected saving: about 55 s of wall time (e2e 21.5 to 5, update 11.6 to
  2.5, doctor_extras_reviewer 10.5 to 0.5, doctor_extras_watchdog 5.4 to
  0.5, scenario 15.1 to 10, skills_exposure and e2e messaging about 3 each).
- Check: time those binaries before and after; full `horch-e2e` passes.

### U2: a fault point for the done push budget

- Files: `crates/horch-core/src/messaging/delivery.rs`,
  `crates/horch/src/cmd/taskledger.rs`,
  `crates/horch/src/cmd/taskledger_watch.rs`,
  `crates/horch-e2e/tests/done_budget.rs`,
  `crates/horch-e2e/tests/fleet18_messaging.rs`.
- Speed: add a `done-push-budget-ms` fault point, read like
  `delivery-probe-ms` in `delivery_probe.rs:13`, with `DONE_PUSH_BUDGET` as
  the default. Use it at the 3 call sites (`taskledger.rs:2670`, `:2703`,
  `taskledger_watch.rs:154`). In the 4 tests (S3 to S6), set it to 2000 ms.
- Anti-tautology: each test asserts that the command took at least 2 s (the
  budget was spent) and less than 10 s (the budget bounded it), not only
  less than the 30 s `LIMIT`. Mutation: ignore the fault and use 20 s; the
  upper bound must fail.
- Expected saving: about 34 s (done_budget 21.1 to 3, fleet18_messaging 20.6
  to 4).

### U3: horch-core unit tests: 3 slow tests and 1 flaky test

- Files: `crates/horch-core/src/harness/codex.rs`,
  `crates/horch-core/src/telemetry/collect.rs`,
  `crates/horch-core/src/routing/quota_probe.rs`,
  `crates/horch-core/src/procid.rs`.
- S1: in `fake_codex` (`codex.rs:1631`), prefix the script with
  `if [ "$1" = warm ]; then exit 0; fi` and warm with `&["warm"]`. Callers
  that pass arguments must not pass `warm`.
- S7: `nfr_02_indexed_fold_matches_the_event_sums` runs 40 cases, not 200.
- S14, S27: `probe_local` takes the deadline as an argument. Keep the
  assertions on the 5 s and 60 s values. Run the timeout case with a 300 ms
  deadline against `sleep 1`, and the slow-but-in-time case with a 2 s
  deadline against `sleep 0.5`.
- F2: allow 2 s, not 1 s, in `a_new_child_started_after_this_process`, and
  name the Linux whole-second boot time in the comment.
- Expected saving: about 21 s (horch-core lib 31.5 to about 10).
- Check: `cargo test -p horch-core --lib` wall time; F2 needs no local
  repro.

### U4: the ledger opens safely when 2 processes create it at once (F1, F4)

- Files: `crates/horch-core/src/taskledger/store.rs`.
- Fix: in `Store::open`, retry `PRAGMA journal_mode = WAL` on `SQLITE_BUSY`
  with a short backoff until `BUSY_TIMEOUT` has passed. The test
  `store_dirlock_excludes_concurrent_writers` needs no change.
- New test in `store.rs`: 8 threads open the same new path at once, 20
  rounds, and every open succeeds.
- Check: run `execution_store --exact store_dirlock_excludes_concurrent_writers`
  200 times alone: 0 failures (now 96 of 200).
  Then run the `fleet18_observability` binary 20 times: 0 failures of
  `g52_concurrent_workers_get_their_own_session_and_transcript` (F4).

### U5: Rust tautology rewrites and duplicate removals

- Files: `crates/horch-core/tests/telemetry_collect.rs`,
  `crates/horch-core/src/taskledger/finish/land.rs`,
  `crates/horch-core/src/routing/decision.rs`,
  `crates/horch-core/tests/context_roster.rs`,
  `crates/horch-core/tests/intake_roster.rs`,
  `crates/horch-core/tests/harness_defaults.rs`,
  `docs/specs/telemetry.md`, `docs/specs/context-policy.md`.
- REWRITE T2 and T3 with the oracle and mutation in the table. Show each
  mutation failing in the commit message.
- REMOVE the 4 D1 tests and their names in the spec tables.
- Check: `cargo test -p horch-core`; `scripts/check-req-coverage.sh`.

### U6: hark Python tautologies

- Files: `skills/hark/engine/tests/recipes_ui/test_ui_keys.py`,
  `skills/hark/engine/tests/fit_analysis/test_fit_acceptance.py`,
  `skills/hark/engine/tests/recipes_ui/test_ui_contract.py`.
- REWRITE T1. REMOVE T4 (4 tests) and T5 (1 test, 14 cases).
- Check: pytest on the 3 files. The hark engine gaps (88 failures) block
  some of these tests now; coordinate with the hark engine owner (Q4).

### U7: skill-intake suite runs in parallel

- Files: `scripts/phase-gate.sh`, `scripts/skill-intake/tests/test_roles.py`,
  `skills/skill-intake/scripts/intakelib/task.py`.
- Speed: in `phase-gate.sh`, run each `scripts/skill-intake/tests/test_*.py`
  module in its own `python3 -m unittest` process, 8 at a time, and fail if
  any fails. Add an env knob for `VERSION_TIMEOUT` (5 s, `task.py:98`) and
  set it to 300 ms in `test_a_horch_that_hangs_past_5_s_is_plain`; keep an
  assertion that the default is 5 s.
- Expected saving: 138 s to about 40 s of gate time (test_roles alone runs
  about 40 s).

### U8: one-page-design Chrome probe (F3)

- Files: `skills/one-page-design/scripts/tests/test_starter_page.py`.
- Fix: before the print, run the found Chrome with `--version`; skip with
  the reason when it does not exit 0.

### U9 (only after Q1 and Q5): parallel runner and oracle tables

- U9a, parallel runner: `.github/workflows/ci.yml`, `justfile`,
  `scripts/phase-gate.sh` (after U7). Needs Q1.
- U9b, oracle tables: `crates/horch-core/tests/baseline_oracles.rs`,
  `crates/horch/tests/baseline_cli.rs`,
  `crates/horch-core/tests/r_routing_assess.rs`, and the files under their
  `oracles/` and `golden/` dirs (T6, T7). Needs Q5. Size: 2 sessions.

## Open questions

- Q1: Adopt `cargo nextest` for CI, `just verify` and the gate? It cuts the
  warm wall time from about 180 s (after U1 to U3) toward 60 to 90 s, and it
  retries and reports flaky tests. It adds 1 tool to install in CI and on
  each machine. Recommendation: yes, after U1 to U4 land.
- Q2: Run the hark pytest suite and the one-page-design suite in CI or the
  gate? Now nothing runs them, and hark has 88 failures that no check sees.
  The hark suite needs numpy, scipy, librosa and soundfile (a 105 s run).
  Recommendation: a gate step that runs when `skills/hark/` changes, once
  the hark engine work is green.
- Q3: Who stops a courier that a killed test leaves? Option 1: the e2e
  harness writes each courier pid to a file and `Harness::new` stops stale
  ones from earlier runs. Option 2: the courier exits after 1 h with no
  reachable herdr. Recommendation: option 1, because it changes test code
  only.
- Q4: The 2 `skills_catalog` failures and the 88 hark failures belong to
  the hark work on this branch. This audit leaves them to that work.
  Confirm.
- Q5: Keep the frozen oracles (T6, T7) as they are, or replace them with
  reviewed spec tables (U9b)? They catch refactor drift, but they bake in
  any bug present at capture and they fail on every intended launch change.
  Recommendation: replace, after the speed and flaky units.
- Q6: Prose pins (`golden_prompts.rs`, `test_skill_docs.py`) are not
  tautological, but every briefing edit needs a matching test edit. Keep
  them? Recommendation: keep; they are the only guard on briefing rules.
