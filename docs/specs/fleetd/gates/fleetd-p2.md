# The gate of Phase 2

The gate: RR-1, RR-2, RR-6, RR-11, FM-3, FM-5 and FM-14 green (`docs/specs/fleetd/design.md`,
Migration plan, item 4). The tests of the gate: a usage-limited worker is not reaped and burns no
attempt, the idle-claim scenario is caught in under 10 min, and a cancel of a task in `running`,
`blocked_on_user` or `waiting_quota` stops the process of a live attempt before the task is
`cancelled`. The release row of Phase 2 is in "What a release does to a run in flight".

A rule is green when each of its tests passes and is off the list of expected failures.

## The commit

- Base: `f063c212`, the tip of `fleet/fleetd-build` when this ticket started. It also holds
  Phase 3 work that landed before this gate.
- Gate: `9a2bf61d`, the child of `f063c212` with the fix of the e2e test helper of
  `p2_leases.rs` (see Linux below). It changes no product code. Each row names this commit.

These tasks of Phase 2 are in the gate commit:

- T-631: the brief of a spawn carries the fleet root (`5f2f016f`, `5eace937`).
- T-656: each text says that a cancelled task is terminal and is not requeued (`cdca67ef`, `5168dc54`).
- T-698: make 2 tests green that the lease task left red (`a8ac6af9`).
- T-816: a stalled task stays stalled until its old process is gone (`17728dec`).
- T-842: the e2e harness never runs a stale `horch` binary (`0cabf6d7`, `913a39ae`).

## The runs

- macOS: macOS aarch64, this Mac. `cargo test --workspace --locked --no-fail-fast` at `f063c212`
  with temporary folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Then, at
  `9a2bf61d`, `cargo build --workspace --bins`, 1 run of each test target of the gate, and 3 runs
  of `p2_leases.rs`.
- Linux: Docker `rust:1.96.1` with the package `bubblewrap`, `--privileged` (bubblewrap makes
  namespaces), Linux aarch64, this Mac, as root with a temporary `HOME`. The source is the tree
  of `9a2bf61d`. On the instruction of the orchestrator, the container runs
  only the tests of this gate, not the whole workspace: `cargo test -p horch-core --lib fleetd::`,
  `cargo test -p horch-core --test fleetd_pass_p2` and `cargo test -p horch-e2e --test p2_leases
  --test p0_liveness --test p2_release --test expected_failures`, after
  `cargo build --workspace --bins`. `p2_leases.rs` runs 3 times.
- Windows: pending CI. No pull request exists for `fleet/fleetd-build`, and
  `gh run list --repo MattMakes/multi-herdr --branch fleet/fleetd-build` is empty on 2026-11-18.
  This Mac cannot compile or run the Windows code.

As in Phase P (`fleetd-pp.md`), a row is green on an OS only with a run of that OS. The Linux
column is a Docker run, not a CI run: a CI run on `ubuntu-latest` replaces it when the pull
request exists.

## The whole workspace on macOS

| Record | ok | failed | ignored |
| --- | --- | --- | --- |
| base `f063c212` | 3724 | 0 | 12 |

No test is red. So no red test is off a list of expected failures. The 12 ignored tests are not
tests of this gate.

The list of expected failures: `crates/horch-e2e/expected-failures/` has no line of Phase 2.
`p0_liveness.txt` holds only its header comment. Each line of a later phase stays.
`cargo test -p horch-e2e --test expected_failures` passes (1 of 1).

The guard tests pass on macOS: `cargo test -p horch-core --test nfr --test arch_scan --test
architecture_doc`.

## The rows

Each test passes on macOS in the whole-workspace run at `f063c212` and in the runs at
`9a2bf61d`. The Linux cells are the Docker run at `9a2bf61d`. Each Windows cell is "pending CI".

| Row | Tests | Commit | macOS (this Mac) | Linux (Docker) | Windows |
| --- | --- | --- | --- | --- | --- |
| RR-1 | `rr1_stopped_worker_is_caught_within_1_lease` (`crates/horch-e2e/tests/p2_leases.rs`, Unix only); `rr1_stopped_worker_is_hung_after_stall_after_s_with_no_activity` (`task_execution/domain/stall.rs`); `renews_only_with_process_heartbeat_and_activity` (`task_execution/domain/lease.rs`) | `9a2bf61d` | green: 3 of 3 pass | pass: 3 of 3; the e2e test asserts the stopped state of the agent | pending CI. The e2e test is `cfg(unix)` (`kill -STOP` has no Windows form, MAP Q17). On Windows the row rests on the 2 unit tests. |
| RR-2 | `rr2_no_sequence_of_facts_leaves_a_task_held_with_an_expired_lease` (`domain/lease.rs`); `rr2_expired_lease_never_leaves_the_task_assigned` (`p2_leases.rs`); `every_held_state_has_an_exit_that_frees_the_task` (`fleet_runtime/domain/ledger.rs`) | `9a2bf61d` | green: 3 of 3 pass | pass: 3 of 3 | pending CI |
| RR-6 | `rr6_no_sequence_of_outcomes_retries_forever` (`application/retry.rs`); `retry_backoff_30s_2m_8m_then_replan_then_escalate` (`domain/retry.rs`); `rr6_task_that_fails_each_attempt_escalates_after_max_attempts` (`p2_leases.rs`) | `9a2bf61d` | green: 3 of 3 pass | pass: 3 of 3 | pending CI |
| RR-11 | `fm5_usage_limit_burns_no_attempt_and_moves_to_fallback` (`application/quota.rs`); `both_pools_limited_waits` and `usage_limit_without_reset_at_waits_for_pool_probe_ok` (`domain/quota.rs`) | `9a2bf61d` | green: 3 of 3 pass | pass: 3 of 3 | pending CI |
| FM-3 | `fm3_idle_claim_is_caught_in_under_10_min` (`crates/horch-e2e/tests/p0_liveness.rs`); `fm3_idle_claim_is_caught_within_stall_after_plus_lease` (`domain/stall.rs`); `background_wait_holds_while_the_agent_child_runs_and_ends_when_it_exits` (`crates/horch-core/tests/fleetd_pass_p2.rs`) | `9a2bf61d` | green: 3 of 3 pass | pass: 3 of 3 | pending CI. On Windows `group_members` gives `NotSupported`, so a background wait holds to its cap. |
| FM-5 | `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` (`p0_liveness.rs`); `usage_limited_is_never_hung` (`domain/stall.rs`); `waiting_approval_ends_the_attempt_and_opens_1_dialog_item_with_the_excerpt` (`application/stall.rs`) | `9a2bf61d` | green: 3 of 3 pass | pass: 3 of 3 | pending CI |
| FM-14 | `fm14_no_second_attempt_starts_while_the_old_process_lives` (`p2_leases.rs`); `fm14_second_lease_for_1_task_is_refused` (`application/lease.rs`); `stop_of_the_old_attempt_comes_before_the_spawn_on_the_fallback` (`fleetd_pass_p2.rs`) | `9a2bf61d` | green: 3 of 3 pass | pass: 3 of 3 | pending CI. Windows has no polite stop, so on Windows the e2e test checks only at most 1 lease and at most 1 open attempt, not the old process in its grace. |
| Test: usage limit | `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` | `9a2bf61d` | green | pass | pending CI |
| Test: idle claim | `fm3_idle_claim_is_caught_in_under_10_min` | `9a2bf61d` | green | pass | pending CI |
| Test: cancel | `cancel_of_a_running_task_stops_its_process_before_the_task_is_cancelled`, `cancel_of_a_blocked_on_user_task_is_immediate`, `cancel_of_a_waiting_quota_task_is_immediate` (`p2_leases.rs`) | `9a2bf61d` | green: 3 of 3 pass | pass: 3 of 3 | pending CI |
| Test: fleet clock | `sleep_does_not_expire_a_lease` (`p0_liveness.rs`, and the unit test of the same name in `domain/lease.rs`) | `9a2bf61d` | green: 2 of 2 pass | pass: 2 of 2 | pending CI |
| Run in flight (release row of Phase 2) | `held_task_with_a_live_worker_on_phase_1_is_not_reaped_after_the_update_and_has_1_lease` (`crates/horch-e2e/tests/p2_release.rs`) | `9a2bf61d` | green | pass | pending CI |

The FM-14 e2e test is strict: `stop_grace_s` is 600, and the old agent ignores `SIGTERM`
(`Agent::stubborn`). It passes after T-816.

The gate of Phase 2 is green on macOS. Each row passes on Linux in Docker. The gate is not complete until CI gives
Linux and Windows runs.

## Linux

The Docker run of the gate tests at `9a2bf61d` (Linux aarch64, `rust:1.96.1`):

| Target | ok | failed | ignored |
| --- | --- | --- | --- |
| `horch-core --lib fleetd::` | 348 | 0 | 1 |
| `horch-core --test fleetd_pass_p2` | 13 | 0 | 0 |
| `horch-e2e --test p0_liveness` | 9 | 0 | 0 |
| `horch-e2e --test p2_leases`, 3 runs | 7, 7, 7 | 0 | 0 |
| `horch-e2e --test p2_release` | 1 | 0 | 0 |
| `horch-e2e --test expected_failures` | 1 | 0 | 0 |

The fault that `9a2bf61d` fixes. At `f063c212`, 2 tests of `p2_leases.rs` failed on Linux in
each of 3 runs: `rr6_task_that_fails_each_attempt_escalates_after_max_attempts` and
`cancel_of_a_blocked_on_user_task_is_immediate`. Each one timed out after 120 s on "the end of the
agent". The cause was the test helper `Agent`, not the product. `Agent::spawn` ran
`/bin/sh -c 'set -m; <command> & echo $!'`. On Debian and Ubuntu `/bin/sh` is `dash`. With no
terminal, `dash` turns job control off, so the agent did not lead its own process group.
`Agent::signal_group` sends a signal only while the agent leads its group, so `Agent::kill` sent
nothing. The same fault made `Agent::freeze` send no `SIGSTOP`, so
`rr1_stopped_worker_is_caught_within_1_lease` passed on Linux with an agent that was not stopped.
On macOS `/bin/sh` is `bash`, and `set -m` works. CI on `ubuntu-latest` also has `dash`.

The fix (`crates/horch-e2e/tests/p2_leases.rs`; the orchestrator extended `Own` to it):

- `Agent::spawn` starts `/bin/sh -c 'exec <command>'` with `process_group(0)`, and a thread
  waits on the child, so no zombie stays. `exec` is needed: else `dash` keeps a shell as the
  agent, and the polite stop ends that shell in place of the stubborn agent (FM-14 failed so).
- `Agent::of` asserts that the agent leads its process group. At `f063c212` this assertion fails
  on Linux in each of the 7 tests.
- `Agent::freeze` waits until `ps` shows the agent in the stopped state (`T`). So the RR-1 e2e
  test proves a real `SIGSTOP` on Linux and on macOS.

No other e2e file uses `set -m`.

The Phase 3 tests `worktree_add_attaches_an_existing_branch_that_is_at_the_base_tip_of_its_row`
and `worktree_add_refuses_an_existing_branch_that_is_not_at_the_base_tip_of_its_row` are not tests
of this gate. They need `bwrap`. With no `bubblewrap` they fail ("bwrap is not on PATH"). In a
container without `--privileged` they fail ("Creating new namespace failed"). They pass in the run
above. CI on Linux needs the `bubblewrap` package, and a runner that lets it make namespaces.

## Deviations from the design and facts of this phase

1. D2, the required fallback: 35 worker personas have no fallback. Each one has a written
   `no_fallback_reason` (confidentiality, trust, safety or capability). A test pins the list of
   35 (T-261). The operator has not confirmed this deviation.
2. `cancelled` is terminal (MAP Q22, T-260). A requeue of a cancelled task is refused by the door.
   `docs/specs/fleetd/preservation-04-behavior.md` row L8 still says that a requeue is possible
   today. This ticket does not change `docs/specs/fleetd/preservation-04-behavior.md`.
3. A task at its attempt limit is `failed`, not held or `escalated` (T-265). The orchestrator gets
   1 QUESTION: requeue it 1 time (its 1 replan), or cancel it.
4. The pinned group of T-260: `TASK_EXITS_OF_TODAY` holds 3 exits that today's code still writes
   and the 12-state table of the design does not have: `running -> done`,
   `running -> escalated` and `verifying -> escalated`. The test
   `task_exits_of_today_are_exactly_the_3_that_the_design_does_not_have` pins the group at 3.
5. The schema step `events_kinds` of T-264: `EVENT_KINDS_VERSION` builds the `events` table again
   from the text of the code. It keeps each row, each `event_id`, the AUTOINCREMENT counter and
   the 2 indexes. `event_kinds_are_pinned_to_the_schema_version` fails when a new event kind has
   no schema step.
6. Rule D of T-269: an idle author of a `verifying` task gets 1 needs-you item
   (`finish_idle:<task>`) and is not stopped. This is interim until Phase 4. Only the close of the
   item on task done has an e2e test. The close on author progress has no test.
7. MAP Q23 option 1 is in the code: a background wait ends at once when the agent child is gone.
   A worker with no recorded agent pid is Unknown and holds to its cap.
8. T-816: a stalled task stays stalled until its old process is gone. Each stop has a sent mark.
   A stop that fails opens 1 `stop_failed` needs-you item.
9. T-268: `horch task cancel` and `horch ledger cancel` refuse a worker pane. The operator token
   for `horch task` is Phase 5.
10. T-842: before T-842 the e2e harness could run a stale `target/debug/horch` with
    `cargo test -p horch-e2e`. T-842 builds `horch` and the fakes before first use. This gate also
    ran `cargo build --workspace --bins` before each partial e2e run.
11. Windows: no polite stop exists, so FM-14 on Windows checks less (row FM-14). The Windows code
    paths of Phase 2 were not compiled on this Mac.

## The open questions of MAP.md

- Q1, `pool_full_use` 0.98 and the key name `replan_limit_s`: open. The operator has not
  confirmed 0.98 or given a value. The code uses 0.98.
- Q23, the background wait: open. The orchestrator chose option 1 (QUESTION #1574b85e), and
  option 1 is in the code (item 7 above). The operator has not confirmed option 1 or chosen
  option 2.

## Flaky tests

None seen. Each test of the gate passed in each run at `9a2bf61d`: macOS (1 run, `p2_leases.rs` 3
runs) and Linux (1 run, `p2_leases.rs` 3 runs). The 2 Linux failures at `f063c212` failed in each
run, so they were a fault, not a flaky test.
T-266 reported 1 flaky test that is not a test of this gate:
`launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out` (`fleetd_pass`, 1 failure
in 6 runs). It passed in the whole-workspace run of this gate.
