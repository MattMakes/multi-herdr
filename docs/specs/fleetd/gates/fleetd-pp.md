# The gate of Phase P

The gate: PL-2 and PL-6 green (`docs/specs/fleetd/design.md`, Migration plan, item 2). The test
of Phase P in "What a release does to a run in flight" is the test of PL-6.

## The commits

- Before the move: `e4017980ff396e3a659762d4c34decfe2bf2a376` (`scripts/pl6/BASE`).
- Pre-fix: `893159c0`, the tip of `fleet/fleetd-build` before this ticket.
- Gate: `7e2430e0`, the child of `893159c0` with the fix of the ipc test below. Each row of
  this gate names the gate commit.

## No CI run exists

No pull request exists for `fleet/fleetd-build`, so CI has no run of `BASE`, of the
pre-fix commit or of the gate commit (`gh run list --branch fleet/fleetd-build` is empty on
2026-11-16). On the instruction of the
orchestrator:

- Linux runs in Docker on this Mac (Linux aarch64), not in CI.
- Windows is pending CI. A separate task gives the Windows proof when the pull request exists.

A Linux or Windows row is green only with a CI run id of its OS. So the Linux row and the Windows
row are not green yet, and the gate of Phase P is not complete until CI gives both.

## The rows

| Gate row | Test | Commit | Run | Result |
| --- | --- | --- | --- | --- |
| PL-2 | `cargo test -p horch-core --test arch_scan` | `7e2430e0` | macOS, this Mac | green: 18 of 18 pass |
| PL-6 on macOS | `scripts/pl6/check.sh` against `baseline-macos.txt` | `7e2430e0` | macOS aarch64, this Mac | green: 2937 tests, no difference |
| PL-6 on Linux | `pl6.py compare` of `BASE` and the gate commit | `e4017980` and `7e2430e0` | Docker `rust:1.96.1`, Linux aarch64, this Mac; no CI run id | not green: pass in Docker (2938 tests, no difference); pending CI |
| PL-6 on Windows | job `test-windows`, `pl6.py compare --gaps scripts/pl6/windows-gaps.txt` | `7e2430e0` | none | not green: pending CI; static source comparison below |
| Run in flight (Phase P) | the test of PL-6 | as the 3 rows above | as the 3 rows above | green on macOS; Linux passes in Docker, pending CI; Windows pending CI |

The list of expected failures of Phase 0: `cargo test -p horch-e2e --test expected_failures`
passes on macOS at `893159c0` (1 of 1).

## Linux

The command in the container: `cargo test --workspace --locked --no-fail-fast`, with a temporary
`HOME` and temporary folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Each
commit has its own named volume for `/src/target`.

| Record | ok | failed | ignored |
| --- | --- | --- | --- |
| `BASE` `e4017980` | 2857 | 9 | 11 |
| pre-fix `893159c0` | 2917 | 10 | 11 |
| gate `7e2430e0` | 2918 | 9 | 11 |

The first compare (`BASE` against pre-fix `893159c0`) found 1 fault:
`crates/horch-platform/tests/ipc.rs` `unix::a_plain_file_is_not_a_socket` failed. On Linux a
connect to a plain file gives `ECONNREFUSED`, not `ENOTSOCK`. The production caller
(`harness/prime.rs`, `listening`) accepts both, as at `BASE`, so behavior did not change. Commit
`7e2430e0` makes the test expect `ConnectionRefused` on Linux and `ENOTSOCK` elsewhere.

The 9 tests below fail at `BASE`, at pre-fix and at gate, with the same reasons. The cause is the
container, not the move. The compare accepts a failure in both records.

- `horch_core::taskledger::finish::complexity::tests::trf_07_clippy_fails_an_over_complex_changed_function_and_passes_a_clean_change`: the image has no `cargo-clippy`.
- `w3_finish_steps::trf_07_complexity_sweep_runs_clippy_on_a_changed_rust_function`: the image has no `cargo-clippy`.
- `horch::cmd::messaging::tests::note_warning::ctx_10_note_warns_with_a_read_only_cache_dir`: the container runs as root, so a read-only folder does not refuse a write.
- `horch::cmd::messaging::tests::note_warning::ctx_10_note_silent_on_unknown_pending_or_error`: same cause as the line above.
- `context_readers::ctx_10_cache_write_failure_keeps_the_reading`: same cause as the line above.
- `telemetry_collect::tel_01_every_agent_is_read`
- `fakes::teardown_stops_the_mail_courier_of_its_state`
- `fleet_start::a_fleet_whose_roster_fails_to_load_creates_no_workspace`
- `horch_e2e::harness::tests::a_harness_of_an_ended_test_process_has_its_processes_stopped`

A CI run on `ubuntu-latest` (x86_64, not root, with clippy) replaces this row when the pull
request exists.

## Windows

This Mac cannot run the Windows tests. The job `test-windows` in `.github/workflows/ci.yml`
keeps `continue-on-error: true` until the Windows proof task records its result.

The static source comparison compared each module of `crates/horch-platform/src/windows/` with the code at `BASE` that
it replaced:

- `process.rs`: `alive`, `start_time`, `started_at`, `started_by` and `filetime_to_system` are
  the code of `procid.rs` at `BASE`. `group_members` gives `NotSupported`, and
  `procid::group_members` maps it to an empty list, as at `BASE`.
- `signal.rs`: `stop_same` opens 1 handle with `PROCESS_TERMINATE` and
  `PROCESS_QUERY_LIMITED_INFORMATION`, and calls `TerminateProcess` with exit code 1 only on a
  start-time match, as `terminate_same` at `BASE`.
- `group.rs`: `Contained` is the job object of `workspace/herdr.rs` at `BASE` (suspended start,
  kill on job close, resume of the main thread). `terminate` gives a bool, and the caller prints
  the same warning as at `BASE`. `kill_child_group` kills the child only, as `kill_group` at
  `BASE`.
- `path.rs`: `set_junction`, `clear_link` and `mklink_junction` are the code of
  `herdr-install/src/win.rs` at `BASE`. The errors are `io::Error` in place of `anyhow`. The
  context text is the same, and the cause follows it after `: ` in the message itself.
- `fs.rs`: `set_mode` and `sync_dir` give `NotSupported`, and `fsx.rs` maps that to `Ok(())`,
  as the no-op of `BASE`. `is_executable` is `is_file` and `make_executable` is a no-op, as at
  `BASE`.
- `ipc.rs`: no local socket, as at `BASE`, where each socket site was `cfg(unix)`.
  `harness/prime.rs` keeps the `BASE` answer: a file at the path counts as listening.
- `machine.rs`: each reading gives `NotSupported`. The host name comes from `COMPUTERNAME`, as at
  `BASE`.

The static source comparison found no change of behavior. It is not a test result.

## Windows gaps

`scripts/pl6/windows-gaps.txt` does not exist yet. The Windows gap tests come from the CI log of
`test-windows`, and no such log exists. The Windows proof task adds the file.

## Flaky tests

None seen. The 2 Linux runs (pre-fix `893159c0` and gate `7e2430e0`) have the same 9 failures,
apart from the fixed ipc test. The 2 macOS runs (pre-fix `893159c0` and gate `7e2430e0`) give no difference.
