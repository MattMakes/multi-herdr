# D21 judge-orphan: a dead judge job never leaves a paid judge CLI running

Branch `ds/judge-orphan`, worker opus-59. Plan:
`ai_docs/plans/design-skills/d21-judge-orphan.md`. This closes the D20
follow-up "a judge job can die while its judge CLI child still runs in its
group" (`ai_docs/reports/design-skills/pid-identity.md`).

## Step 1: the CLI runs in the job's group (no change)

- `evaluation/scheduler.rs` `schedule` starts the job with
  `runtime::process::spawn_detached`, which calls `setsid` in the child. The
  job leads a new session and a new group, so the pgid is the job's pid.
- `crates/horch/src/dataset/judge_job.rs` `run_judge` spawns the CLI from
  `headless_command` with no group or session change, so the CLI inherits the
  job's group. `judge_job.rs` did not change.
- Not covered: a process that the CLI itself moves to another group (for
  example a tool child that leads its own group). horch cannot find it
  through the job's group.

## Step 2: the Lost path (orchestrator's option 3)

The plan's rule was "no process has the pid → `killpg`". I asked the
orchestrator before I implemented it, because D20 found a gap. A heartbeat
can be old (for example after coordinator downtime). The job's group can
empty, and a later program can get the pid, lead a group and die. A
daemon that calls `setsid`, forks and exits leaves exactly that. A `killpg`
then ends a foreign group. The orchestrator chose option 3: no `killpg`,
and no age bound. horch lists the group members and kills only the members
that started at or before the last heartbeat.

Why this is correct: the order of events for a foreign group is
last heartbeat < job death < pid reuse < foreign leader start < foreign
member start. Every orphan of the job started while the job was alive.

- `scheduler.rs` `kill_lost_job(job_dir) -> LostKill { job, killed, kept }`:
  1. `kill_job(&hb)` as before: a job that still runs as itself is killed
     with its group.
  2. A pid that a process has now (another program, or the job without a
     start time) gets no signal.
  3. Else, for each pid in `procid::group_members(hb.pid)`: when
     `procid::started_by(pid, mtime of heartbeat)` is true, `kill_member`
     checks `procid::is_same(pid, start)` and `getpgid(pid) == pgid` again
     and then sends SIGKILL to that pid only. All other members go to
     `kept`.
- The time is the `heartbeat` file mtime, not `hb.at`: `hb.at` has 1 s
  precision (`clock::stamp`).
- `competition/judging.rs` Lost path calls `kill_lost_job` and logs to
  stderr: `horch: judge attempt N of round R was lost; killed its orphans
  [..]; kept [..], which started after its last heartbeat`. It logs only
  when the group had members.
- `procid.rs` (given to this unit by the orchestrator):
  - `group_members(pgid) -> Vec<u32>`: macOS `proc_listpgrppids` (buffer of
    4096 pids, zeroed, so the byte-or-count return value is safe); Linux
    scans `/proc/*/stat` field 5; other platforms give an empty list.
  - `started_by(pid, SystemTime) -> bool`: macOS compares `started_at`
    (microseconds). Linux compares in clock ticks on `CLOCK_BOOTTIME`. It
    moves `t` by the distance from now, so the whole-second `btime` cannot
    make a foreign start look earlier than `t`. An unknown start time is
    `false`.
  - Linux `stat_field` is a shared parser for fields 5 and 22.
- Windows: `group_members` is empty and `kill_member` sends nothing, so
  only `kill_job` applies. That is the old behaviour.

## Step 3: tests

- `crates/horch-core/tests/judging.rs`
  `jdg_08_lost_job_kills_its_orphaned_judge_cli` (through `poll`): a `sh`
  leader in a new session (`spawn_detached`) starts `sleep` A (the fake
  judge CLI, which writes its pid). The test writes the heartbeat. The
  leader then starts `sleep` B and exits, and the test reaps it. After
  `poll`: `judge.failed {cause: lost}`, A is gone, and B still runs. The
  test cleans up both with `is_same` checks.
  - Before the fix it failed: `the orphaned judge CLI still runs`.
  - A `killpg` would also kill B, so the test also catches option 1.
- `procid.rs` unit tests: `group_members_lists_this_group` (this test
  process and its child; 0 and `u32::MAX` give nothing),
  `started_by_orders_a_start_with_a_time`, and field 5 in
  `stat_field_22_after_a_command_with_spaces`.
- The test is in `tests/`, because the CMP-16 audit
  (`horch-e2e/tests/judge.rs` `cmp_16_only_judge_detached`) rejects
  `setsid`, `process_group` and `spawn_detached` in `src/` files that are
  not on its list. `procid.rs` uses none of these words.

## Checks

- The full gate (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`)
  ran once before the rebase, as its own command: GATE GREEN.
- After the rebase on `design-skills`, the merge-train checks ran: the
  workspace build, the touched test files, clippy with `-D warnings`, and
  `rustfmt --check`. Never under `git rebase -x`.

## Gotchas and follow-ups

- The Linux code compiles and runs only on Linux. This machine has no Linux
  target installed, so CI (or a Linux host) is the first run of
  `group_members`, `started_by` and `stat_field` there.
- Recent Linux kernels count start times on `CLOCK_BOOTTIME`. Older
  kernels count `CLOCK_MONOTONIC`, which differs after a suspend. Then
  an orphan can look newer than it is, and is kept (safe side).
- A job that dies before its second heartbeat (within 2 s of its start) has
  a CLI that started after the only heartbeat. That CLI is kept. This is
  the safe side, and it is logged.
- A member that the CLI starts after the last heartbeat is kept as well
  (the same rule).
