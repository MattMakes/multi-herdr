# D20 pid-identity: never signal or trust a reused pid

Branch `ds/pid-identity`, worker opus-50. Plan:
`ai_docs/plans/design-skills/d20-pid-identity.md`.

| Step | Commit | State |
|---|---|---|
| 1. Shared helper; fake-herdr uses it | `Core: Add procid, a pid with its start time, and use it in fake-herdr` | Done |
| Site 1. Judge job `Lost` path | `Evaluation: Kill a judge job only while its pid is still the job` | Done |
| Site 2. Prime daemon SIGTERM | `Harness: Stop a Prime daemon only when it is the one this launch started` | Done |
| Site 3. `DirLock` and collector lock liveness | `Core: Treat a lock holder whose pid has another start time as dead` | Done |
| Site 4. Other sites | `Evaluation: Kill a reaped gate's group only while no process has its pid` | Done |

The gate (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`) runs on
every commit with `git rebase --exec`. See "Gate" at the end.

## The helper: `crates/horch-core/src/procid.rs`

- `start_time(pid) -> Option<u64>`: an opaque value that you compare only
  for equality. On macOS it is `proc_pidinfo(PROC_PIDTBSDINFO)`
  `pbi_start_tvsec/usec`, in microseconds since the epoch. On Linux it is
  field 22 of `/proc/<pid>/stat`, in clock ticks since boot. The parse
  starts after the last `)`, because the command can hold spaces and `)`.
  On other platforms (Windows) it is `None`.
- `started_at(pid) -> Option<SystemTime>`: the same value on the wall clock.
  Only the Prime check uses it. On Linux it is `btime` plus ticks divided
  by `_SC_CLK_TCK`, so it can be up to 1 s off.
- `alive(pid, Option<start>)`: the pid exists, and when a start time is
  recorded, it is the same. With no recorded start time (an old file), or
  no readable start time, the result is "the pid exists", as before.
- `is_same(pid, start)`: the check before every signal. A start time that
  it cannot read is not a match.
- `is_same_group(pgid, start)` (unix): the leader runs with that start time
  and still leads the group. Or the leader is gone and the group still has
  members (`kill(-pgid, 0)`). POSIX keeps the id from reuse while the group
  has members.
- `raw()` refuses 0 and any pid above `i32::MAX`. A cast of such a pid to
  `pid_t` gives 0 or a negative number: a signal to a group, or to every
  process.
- Only `libc` is used. There are no new crates.

### fake-herdr and the e2e harness (D19's fix moved)

D19 (commit 648594b) compared `ps -o lstart` strings in
`crates/horch-e2e/src/process.rs`. Now:

- `fake-herdr` records `procid::start_time(pid)` (a number) in
  `state["started"][pane]`. `pane close` signals only when
  `procid::is_same_group` is true.
- `harness.rs` teardown (`reap`) uses `procid::is_same_group` for the
  recorded panes. This is a change of 3 lines. D19's second merge does not
  touch this file (it changes only `tests/fakes.rs`, `tests/lifecycle.rs`
  and its report), so the units do not overlap.
- `process.rs` keeps only `processes()` (now `pid,pgid,command`) and
  `signal_groups`. `started()`, `is_same_group` and the `lstart` parse are
  removed. The D19 unit test cases moved to procid tests.
- `crates/horch-e2e/Cargo.toml`: `horch-core` moved from
  `[dev-dependencies]` to `[dependencies]`, because the fake binaries now
  link it. `check-deps.sh` does not check `horch-e2e`.

### Gotcha: CMP-16 audit

`crates/horch-e2e/tests/judge.rs` `cmp_16_only_judge_detached` fails for
any `src/` file that contains `process_group`. So the procid group tests
are in `crates/horch-core/tests/procid.rs`, which the audit skips. The
audit test is not changed.

## Site 1: judge job (`evaluation/scheduler.rs`, `competition/judging.rs`)

- `Heartbeat` has a new field `started: Option<u64>`
  (`serde(default, skip_serializing_if = "Option::is_none")`), so old
  heartbeats still parse. The job writes it
  (`crates/horch/src/dataset/judge_job.rs` `beat`). That file is the
  heartbeat writer, outside the plan's file list. The plan requires "the
  heartbeat records the job's start time", so the change is 1 field.
- `job_facts.pid_alive` is now `procid::alive(pid, started)`. A stale
  heartbeat whose pid was reused is not alive, and `decide` gives `Lost`.
- `kill_job(&Heartbeat) -> bool`: it signals only when `started` is
  `Some` and `procid::is_same` is true. A heartbeat without a start time is
  never signalled.
- `Lost` path: it calls `kill_job` only when `facts.pid_alive` is true. It
  never signals a pid that `decide` found dead.
- The `Running` overdue path calls `kill_job(&hb)` too. Its signature
  changed, so this line changed.
- Tests:
  - `scheduler.rs` `kill_job_only_kills_the_same_process`: no start time,
    a wrong start time, and a reaped child each give no signal. The right
    start time kills the test's own `sleep`.
  - `scheduler.rs` `heartbeat_shape`: an old heartbeat parses.
  - `tests/judging.rs` `jdg_08_lost_job_kills_only_the_same_process`
    (integration, through `poll`). A stale heartbeat names a live `sleep`
    with another start time. The result is `judge.failed {cause: lost}`
    and the `sleep` still runs. The old code sent SIGKILL to it. Then a
    stale heartbeat with the right start time kills it.
  - `jdg_08_overdue_job_times_out` now writes the `sleep`'s start time.
    The other test heartbeats write `started: None`. The `FakeLauncher`
    heartbeat names the test process itself. Before this change, a `Lost`
    in those tests would SIGKILL the test runner's group. Now it cannot.

## Site 2: Prime daemon (`harness/prime.rs`)

The plan said "a daemon pid read from a file". That is not correct: the pid
comes from a live `prime-agent status --json`. No start time is recorded
for the daemon, and `prime-agent` is a node script, so a name check sees
only `node`. The orchestrator chose this rule. `Daemon::finish` sends
SIGTERM only when both of these are true:

1. 2 `status` calls name the same pid with the same `procid::start_time`.
2. That process started after `Daemon::install`, with `started_at + 1 s >=
   installed`. The 1 s is Linux rounding.

Otherwise it logs `horch: left Prime daemon pid N running: <why>` and does
not send a signal. On Windows only rule 1's pid check applies (there is no
start time), so the behaviour there is as before.

- Tests: `only_the_same_daemon_started_by_this_launch_is_stopped` (a table
  for `may_stop`). `finish_stops_only_a_daemon_this_launch_started` uses a
  `sh` script as the `status` program, which names a `sleep` child. With
  `installed` after the child's start, the child is left running. With
  `installed` before it, the child gets SIGTERM.
- Remaining risk: Prime's own registry can keep a stale entry for this
  socket after the daemon dies. If the OS gives that pid to a process
  started after this launch, both checks pass, and that process gets
  SIGTERM. For this, the daemon must die before the pane ends, and its pid
  must be reused inside the pane's lifetime. horch cannot close this gap
  without a start time from Prime itself.

## Site 3: lock liveness (`fsx.rs`, `telemetry/lock.rs`)

- `LockOwner` (in `<name>.lock/owner`) has a new field `started:
  Option<u64>`. `LockInfo` (in `collector.json`) has a new field
  `pid_start: Option<u64>`. The name `started_at` was already used for a
  time string. Both fields are `serde(default)`, so old files still read.
  A missing value means "the pid exists", as before.
- `is_stale` and `holder` use `procid::alive`.
- The module doc in `telemetry/lock.rs` said that start times cannot be
  read. It now describes the new rule.
- Tests: `dirlock_breaks_an_owner_whose_pid_was_reused` and
  `spc_01_a_reused_pid_is_a_stale_lock` use the test process's own pid
  with a wrong start time. The lock is stale and is broken. An old file
  without a start time is still held.

## Site 4: every other site

Search: `rg -n 'kill\(|killpg|SIGKILL|SIGTERM|tasklist|taskkill' crates/*/src`, and test code.

| Site | Decision |
|---|---|
| `evaluation/validator.rs` `kill_group` after `try_wait` returned `Some` (gate passed or failed) | **Fixed**, with the orchestrator's approval. The leader is reaped, so its pid is free. The new `kill_reaped_group` calls `killpg` only while no process has the pid (`fsx::pid_alive` is false; EPERM counts as alive). POSIX gives no new process a pid while a group with that id exists. So if the group has members, it is still the gate's group, and the stragglers are killed. A process that has the pid is another program, and gets no signal. Tests: `a_reaped_leaders_pid_that_runs_again_is_not_the_gate` (unit) and `tests/vcs.rs` `cmp_09_a_passed_gates_background_child_is_killed` (a passed gate leaves a `sleep` in its group, and the `sleep` dies). |
| `evaluation/validator.rs` `kill_group` at the timeout | Safe: the child is not reaped yet (`child.wait()` comes after). |
| `runtime/machine.rs:221`, `harness/headless.rs:177`, `routing/quota_probe.rs:70,207,251`, `dataset/judge_job.rs:181` (`Child::kill`) | Safe: each child is held unreaped, and `wait()` comes after `kill()`. |
| `horch/src/cmd/telemetry.rs:49` | Not a signal send: it installs a SIGTERM handler. |
| `fsx::pid_alive` (`tasklist` on Windows) | Kept as "the pid exists". Now only `procid::alive` and tests call it. |
| `crates/horch-e2e/tests/e2e.rs` `stop_collector_in` | **Fixed** (test code). It sent SIGTERM with `/bin/kill` to the pid in `collector.json`. A collector that had ended leaves that file, so this could end another program, the same defect as D19's. It now signals only when `pid_start` is present and `procid::is_same` is true. `hold_lock` writes the test's own pid with no `pid_start`, so the test runner can no longer signal itself. |
| `crates/horch-e2e/src/harness.rs` `reap`, groups found by command line | Safe enough: the group ids come from a live `ps` snapshot of processes whose command names the harness root. The race is from the snapshot to the kill only. |
| `crates/horch-e2e/src/bin/fake-herdr.rs` Windows `taskkill` | Kept: no start time on Windows. Unix uses `is_same_group`. |
| `crates/horch-e2e/src/bin/fake-prime.rs` `alive` (`kill -0`) | Kept: it filters the fake registry for `status`, sends no signal, and only test code uses it. |

## Windows

There is no start time (`GetProcessTimes` needs a Windows crate, which is a
new dependency). Every check falls back to the old behaviour there: the pid
alone decides.

## Not done and follow-ups

- The Prime risk in Site 2 stays until Prime reports a daemon start time.
- A judge job can die while its judge CLI child still runs in its group.
  That job is now recorded `Lost` and gets no signal. Before, its group
  got SIGKILL. The plan requires "never signal a pid that `decide` found
  dead". The job enforced the CLI's timeout, so the orphaned CLI runs until
  it finishes by itself. `procid::is_same_group(pid, started)` would kill
  that group safely in most cases. But a stale heartbeat can be old: the
  original group can empty, and a later group can get the same id and
  lose its leader. So I did not add it. The orchestrator can decide on a
  follow-up.
