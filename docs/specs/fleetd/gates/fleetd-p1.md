# The gate of Phase 1

The gate: RR-4, RR-7, OR-2, OR-4, OR-5, OR-6, PL-1, PL-3, FM-4, FM-7, FM-11, FM-12, FM-22, FM-34,
FM-44, the part of FM-13 that Phase 1 can do, and the old-watchdog test of the release row of
Phase 1 (`docs/specs/fleetd/design.md`, Migration plan, item 3). A rule is green when each of its
tests passes and is off the list of expected failures.

## The commits

- Pre-fix: `97236dda15a5ca2a899cd2e9b43706f5edae50dd`, the tip of `fleet/fleetd-build` on
  2026-11-17 before this ticket. The 2 full runs below ran at this commit.
- Gate: `34f2a88c`, the child of `97236dda` with the fix of the 10,000 folders test below. The
  fix changes only the test `scans_of_10000_folders_...` and 1 helper of it in
  `crates/horch-e2e/tests/p1_pass.rs`. Each row names the commit of its runs: `97236dda` for the
  full runs, `34f2a88c` for the runs of the 10,000 folders test.

## No CI run exists

No pull request exists for `fleet/fleetd-build`, so CI has no run of the gate commit
(`gh run list --repo MattMakes/multi-herdr --branch fleet/fleetd-build` is empty on 2026-11-17).
On the instruction of the orchestrator:

- macOS runs on this Mac (macOS aarch64).
- Linux runs in Docker on this Mac (image `rust:1.96.1`, Linux aarch64), not in CI.
- Windows is pending CI, because no pull request exists yet.

A Linux or Windows row is green only with a CI run id of its OS. So no Linux row and no Windows
row is green yet, and the gate of Phase 1 is not complete until CI gives both.

## The runs

| Run | Command | ok | failed | ignored |
| --- | --- | --- | --- | --- |
| macOS aarch64, this Mac, `97236dda` | `cargo test --workspace --locked --no-fail-fast` | 3206 | 0 | 12 |
| Linux aarch64, Docker `rust:1.96.1`, `97236dda` | `cargo test --workspace --locked --no-fail-fast` | 3198 | 11 | 12 |
| macOS aarch64, this Mac, `34f2a88c` | the 10,000 folders test, 5 runs alone | 5 | 0 | 0 |
| Linux aarch64, Docker `rust:1.96.1`, `34f2a88c` | the 10,000 folders test, 5 runs alone | 5 | 0 | 0 |

Both runs use a temporary `HOME` (Linux only) and temporary folders for `HORCH_DATA_DIR`,
`HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. The Linux run uses the source of the gate commit from
`git archive`, a named volume for `/src/target` and a named volume for the cargo registry. Both
volumes are removed at the end.

The list of expected failures: `cargo test -p horch-e2e --test expected_failures` passes on
macOS and on Linux. No line of Phase 1 is left in `crates/horch-e2e/expected-failures/`. The line
`fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it` (`p0_rescue.txt`) is
the part of FM-13 of a later phase, and it stays.

## The rows

"pending CI" means: no CI run id exists for that OS. "unix only" means: the test file has
`#![cfg(unix)]`, so a Windows build has no test for the row.

| Row | Test | Commit | macOS | Linux (Docker, no run id) | Windows |
| --- | --- | --- | --- | --- | --- |
| RR-4 | `rr4_tasks_complete_with_all_file_watch_events_dropped` (`crates/horch-e2e/tests/p1_pass.rs`) | `97236dda` | pass | pass; pending CI | pending CI; unix only |
| RR-7 | `rr7_fleetd_killed_mid_run_ends_as_a_clean_run` (`p1_pass.rs`) | `97236dda` | pass | pass; pending CI | pending CI; unix only |
| OR-2 | `every_status_change_has_an_event_with_a_cause` (`crates/horch-core/src/taskledger/finish/mod.rs`), `fitness_status_writes_go_through_transition` (`crates/horch-core/tests/arch_scan.rs`) | `97236dda` | pass | pass; pending CI | pending CI |
| OR-4 | `pacing_holds_a_pool_ahead_of_its_window_and_frees_one_on_pace` (`crates/horch-core/src/fleetd/fleet_runtime/domain/health.rs`) | `97236dda` | pass | pass; pending CI | pending CI |
| OR-5 | `low_disk_probe_pauses_dispatch_and_normal_probe_resumes`, `pres_06_s12_unknown_probe_neither_pauses_nor_passes` (`health.rs`) | `97236dda` | pass | pass; pending CI | pending CI |
| OR-6 | `self_update_restarts_the_running_fleetd_before_it_replaces_itself` (`crates/horch/src/cmd/selfupdate.rs`), `check_restarts_an_older_fleetd` (`crates/horch/src/cmd/fleetd.rs`), `newer_schema_9_is_refused` (`crates/horch-core/tests/taskledger_schema_v8.rs`) | `97236dda` | pass | pass; pending CI | pending CI |
| PL-1 | `cargo test --workspace --locked` | `97236dda` and `34f2a88c` | pass: 3206 ok, 0 failed | 11 failed in Docker: 10 by the container, 1 fixed in `34f2a88c`, see Linux below; pending CI | pending CI |
| PL-3 | the `NotSupported` tests of `crates/horch-platform/tests/`: `watchdog_ping_is_not_supported_off_linux` (macOS, Windows), `junctions_are_not_supported_off_windows`, `ensure_user_path_is_not_supported_off_windows` (unix), `not_supported_names_its_concern_and_os`, `not_supported_converts_to_io_unsupported` (each OS), and the `*_not_supported_on_windows` tests, for example `counts_are_not_supported_on_windows` (Windows) | `97236dda` | pass | pass; pending CI | pending CI |
| FM-4 | `fm4_dead_agent_ends_its_attempt_within_30_s` (`crates/horch-e2e/tests/p0_liveness.rs`) | `97236dda` | pass | pass; pending CI | pending CI; unix only |
| FM-7 | `fm7_killed_watchdog_does_not_stop_the_reconcile` (`crates/horch-e2e/tests/p0_supervision.rs`) | `97236dda` | pass | pass; pending CI | pending CI; unix only |
| FM-11 | `fm11_fleetd_aborted_after_the_commit_does_each_effect_once_on_restart` (`p1_pass.rs`), `fleetd_killed_between_commit_and_effect_does_the_effect_once_on_restart` (`crates/horch-core/tests/fleetd_pass.rs`) | `97236dda` | pass | pass; pending CI | pending CI; `p1_pass.rs` is unix only |
| FM-12 | `fm12_hung_fleetd_is_restarted_on_each_os` (`crates/horch-e2e/tests/p0_fleetd_hang.rs`), `job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted` (`crates/horch-e2e/tests/p1_jobs.rs`) | `97236dda` | pass | pass; pending CI | pending CI; `p1_jobs.rs` is unix only |
| FM-13 (part) | `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` (`crates/horch-e2e/tests/p0_rescue.rs`) | `97236dda` | pass | pass; pending CI | pending CI; unix only |
| FM-22 | `fm22_orphan_process_and_pane_after_a_reap_are_stopped` (`crates/horch-e2e/tests/p1_orphans.rs`), `i5_orphan_process_or_pane_is_stopped` (`crates/horch-core/src/fleetd/task_execution/application/commands.rs`) | `97236dda` | pass | pass; pending CI | pending CI; `p1_orphans.rs` is unix only |
| FM-34 | `fm34_daemon_restart_on_a_new_binary_keeps_the_run` (`p0_supervision.rs`) | `97236dda` | pass | pass; pending CI | pending CI; unix only |
| FM-44 | `needs_you_item_written_twice_by_key_is_1_item_and_resolves_with_its_row`, `item_not_open_is_removed_30_days_after_it_ended` (`crates/horch-core/src/fleetd/ideas/application/needs_you.rs`) | `97236dda` | pass | pass; pending CI | pending CI |
| Old watchdog (release row of Phase 1) | `update_with_a_live_worker_and_a_live_old_watchdog_leaves_1_control_path` (`crates/horch-e2e/tests/p1_release.rs`) | `97236dda` | pass | pass; pending CI | pending CI; unix only |
| Test: 5 min job | `job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted` (`p1_jobs.rs`) | `97236dda` | pass | pass; pending CI | pending CI; unix only |
| Test: 10,000 folders | `scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart` (`p1_pass.rs`) | `34f2a88c` | pass: 5 of 5 | pass: 5 of 5; pending CI | pending CI; unix only |
| Test: job backoff and requeue | `job_past_its_limit_runs_again_after_its_backoff_and_after_3_abandoned_runs_goes_to_you` (`p1_jobs.rs`) | `97236dda` | pass | pass; pending CI | pending CI; unix only |

## Linux

11 tests fail in the container. 9 of them are the container failures of the gate of Phase P
(`docs/specs/fleetd/gates/fleetd-pp.md`, section Linux), with the same causes: the image has no
`cargo-clippy`, and the container runs as root.

- `horch_core::taskledger::finish::complexity::tests::trf_07_clippy_fails_an_over_complex_changed_function_and_passes_a_clean_change`
- `w3_finish_steps::trf_07_complexity_sweep_runs_clippy_on_a_changed_rust_function`
- `horch::cmd::messaging::tests::note_warning::ctx_10_note_warns_with_a_read_only_cache_dir`
- `horch::cmd::messaging::tests::note_warning::ctx_10_note_silent_on_unknown_pending_or_error`
- `context_readers::ctx_10_cache_write_failure_keeps_the_reading`
- `telemetry_collect::tel_01_every_agent_is_read`
- `fakes::teardown_stops_the_mail_courier_of_its_state`
- `fleet_start::a_fleet_whose_roster_fails_to_load_creates_no_workspace`
- `horch_e2e::harness::tests::a_harness_of_an_ended_test_process_has_its_processes_stopped`

2 failures are new since the gate of Phase P:

- `fleetd_scans::an_unreadable_worktree_folder_is_not_missing`
  (`crates/horch-core/tests/fleetd_scans.rs`). Cause: a root user ignores file modes, so a
  folder with mode `000` is readable and the test proves nothing as root. The same test binary
  fails as root and passes as the user `nobody` (`setpriv --reuid=65534`) in the same container.
  CI does not run as root. The test stays as it is.
- `p1_pass::scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart`.
  At `97236dda` it failed 4 of 4 runs: 1 in the full run and 3 runs alone. The heartbeat part
  passed. The assert "the scan of worktree_folders ended a lap of 10000 folders before the kill"
  failed: a stat on Linux is faster than on macOS, so with `scan_budget_ms = 5` 1 lap of 10,000
  folders ended in the 4 heartbeats before the kill. The test depended on the speed of the host.
  The scan stops at its budget, so the product was correct.

  The fix (`34f2a88c`, on the instruction of the orchestrator, which added `p1_pass.rs` to the
  Own of this ticket): the test kills fleetd after 3 passes that each moved each folder cursor,
  not after a fixed number of heartbeats, and uses `scan_budget_ms = 1`. The first pass after the
  restart must move each cursor past its stored key, in the same lap. The 4 heartbeats and
  `horch fleetd check` now run on the second fleetd. A deliberate break (each new process starts
  each scan at the first folder, in `cursors_store::load`) makes the test fail: "the scan of
  outboxes is at Some("S-00005/A1"), not after its cursor Some("S-00017/A1")". The break is
  not committed. After the fix the test passes 5 of 5 runs on macOS and 5 of 5 runs on Linux in
  Docker.

A CI run on `ubuntu-latest` (x86_64, not root, with clippy) replaces the Linux column when the
pull request exists.

## Windows

This Mac cannot run the Windows tests. Each Windows result is pending CI. The e2e test files of
the gate, except `p0_fleetd_hang.rs`, have `#![cfg(unix)]`: `p1_pass.rs`, `p0_liveness.rs`,
`p0_supervision.rs`, `p1_jobs.rs`, `p0_rescue.rs`, `p1_orphans.rs` and `p1_release.rs`. A Windows
CI run therefore has no test for the rows RR-4, RR-7, FM-4, FM-7, FM-13 (part), FM-34, the old
watchdog row, the 5 min job, the 10,000 folders and the job backoff, and for the e2e half of
FM-11, FM-12 and FM-22. The unit and core tests of those rules run on Windows.

## Tasks after the map

These tasks landed on `fleet/fleetd-build` after the map of Phase 1. They belong to the phase.

| Task | What it gives | Commit |
| --- | --- | --- |
| T-309 | The test `lock_is_free_after_drop` (`crates/horch-platform/tests/lock.rs`) is stable: the second take waits up to 5 s for a child that holds a copy of the lock file between fork and exec. | `e7b275ea` |
| T-313 | No test reads the real load of the machine: the spawn test of `crates/horch/tests/w3_ledger_cli_pins.rs` uses a machine fixture and a test `fleet.toml`. | `262d8b18` |
| T-322 | The run duties of part 2 of the pass have a work budget and cursors (PL-5): 4 paged duties keep a cursor in `pass_cursors`, and the whole-run duties do 1 count query first. | `a72b66eb` |
| T-325 | `service::ensure` replaces `service::install`: each install loads each label with its current definition, so an install is safe to run again. | `31d9628e` |
| T-414 | The pane of a reaped worker that ends on the polite stop is closed when a fact proves it, else fleetd keeps the pane and opens 1 `pane_kept` item. `Target::Process` uses `ProcessScope`. | `97236dda` |

## Deviations and open points

- `horch job requeue` trusts `HORCH_ROLE` until the command channel of Phase 5. The orchestrator
  waived finding F6 of T-86 for this.
- The pane proof of T-414 is a durable file, `<fleet root>/run/stops/<pid>-<start>.json`, and
  not a ledger row.
- `horch ledger watch --once --json` has no verdict `dead` now (T-88).
- The tests of `crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs` are
  unix only.
- Each Windows result is pending CI, because no pull request exists yet. Most e2e rows have no
  Windows test (section Windows).
- Each Linux result comes from Docker on this Mac and has no CI run id, so it is pending CI.
- The test of the row "Test: 10,000 folders" changed in this ticket (section Linux).
- Schema: the fleetd schema is version 8 on this branch. The main branch also has a version 8. A
  later merge renumbers the version of this branch to 9.

## Flaky tests

None seen. The macOS run had 0 failures. Each Linux failure repeats with the same cause. The
10,000 folders test failed on each Linux run at `97236dda`, so it was not flaky but dependent on
the speed of the host; `34f2a88c` fixes it.
