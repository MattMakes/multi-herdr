# 04: Restart a watchdog that is alive but hung

Status: proposed. Target: `main` at `06930d95`. Source of the idea: fleetd at `9bc3bf70`.
Report id: FM-12. Size: S.

Labels: `CODE` (read at the pinned commit), `MEASURED` (read-only query), `REPORT` (a report
says it, not checked), `NOT VERIFIED`. Path short forms: `TL` is
`crates/horch-core/src/taskledger`, `CMD` is `crates/horch/src/cmd`, and `FLEETD` is
`crates/horch-core/src/fleetd/fleet_runtime` at `9bc3bf70`. `Stop` here is a `process stop` (a signal).

## Problem

The problem is real on MAIN. No incident is recorded, so its frequency is unknown.

Each active run has 1 watchdog process, `horch ledger watch`. It finds dead workers, wakes the
orchestrator and retries failed cleanups. If this process stops its work but stays alive, no
stall is seen and no wake is sent. No `horch` command replaces it or reports it. The fleet stops
with no message, and the operator sees only a `last checkin` time that does not move.

The exact cause has 3 parts:

1. `ensure` calls `needs_restart` only when no live process holds the watchdog lock. A hung
   watchdog holds the lock, so `ensure` goes to `keep_or_replace`, which checks only the binary.
   `CODE CMD/taskledger_watch.rs:396`, `CODE CMD/taskledger_watch.rs:428`.
2. `needs_restart` returns `stale && !alive`, so it is false for a live process.
   `CODE TL/watchdog.rs:692`. R6 names only this line. A change to this line alone does not fix
   the fault, because of part 1.
3. `LockHolder::stop` sends only SIGTERM. The handler only sets a flag, and the loop reads the
   flag between passes. A hung pass never reads it. `CODE TL/watchdog.rs:578`,
   `CODE CMD/taskledger_watch.rs:28`, `CODE CMD/taskledger_watch.rs:377`.

Who starts the watchdog: `run()` calls `ensure` after each `horch` command, except `horch worker`,
`horch install` and `horch ledger watch`. `CODE crates/horch/src/main.rs:801`,
`CODE crates/horch/src/main.rs:883`. `ensure` reads only the runs of the current project.
`CODE CMD/taskledger_watch.rs:413`.

Who can see that it is stale: only `horch ledger status`, which prints the checkin time.
`CODE TL/status.rs:488`. **Nothing runs when no `horch` command runs.** The worker supervisor
writes its own checkin but does not call `ensure`. `CODE TL/supervisor.rs:91`. If every agent
waits for a wake, no process detects the hang. This fix does not close that gap (see Q1).

## Evidence on main

No count of hung watchdogs exists. The ledger has no event for this fault, and the restart
warning goes only to the stderr of the command. `CODE CMD/taskledger_watch.rs:445`. No real
example exists, so this spec gives no task id or worker id for one.

All queries ran with `sqlite3 -readonly` on `~/.horch/ledger.sqlite3` on 2026-11-22 at 23:11Z.
The `checks` table covers 2026-10-31 to 2026-11-22.

1. `MEASURED` Active runs with a stale checkin and a task in flight: **0**.
   ```sql
   SELECT count(*) FROM runs r WHERE status='active' AND watchdog_checkin_at IS NOT NULL
   AND (julianday('now')-julianday(watchdog_checkin_at))*86400 > 2*check_every_s
   AND EXISTS (SELECT 1 FROM tasks t WHERE t.run_id=r.run_id AND t.status IN ('running','landing'));
   ```
2. `MEASURED` Of 19 active runs, 13 have a checkin at most 16 s old, 4 have no checkin, and 2 are
   stale. The 2 stale runs are not this fault. `mining-overhaul` (PID 53628) and `devblog-studio`
   (PID 15895) last checked in on 2026-11-07, have no task in flight, and `ps -p` finds neither PID.
3. `MEASURED` Gaps between 2 check passes of 1 run: 1920 gaps, 33 over 1200 s, 14 over 3600 s,
   largest 427304 s (`godot-port`, from 2026-11-01T00:57:31Z). This is an upper limit only. A gap
   also occurs when the machine sleeps, when no worker is due, or when the watchdog is dead.
   ```sql
   WITH p AS (SELECT DISTINCT w.run_id, strftime('%s', c.at) AS t FROM checks c
              JOIN workers w ON w.worker_id=c.worker_id),
   g AS (SELECT t - lag(t) OVER (PARTITION BY run_id ORDER BY t) AS gap FROM p)
   SELECT count(*), sum(gap>1200), sum(gap>3600), max(gap) FROM g WHERE gap IS NOT NULL;
   ```
4. `MEASURED` The slowest work in a pass is `git worktree remove`. In 474 passed cleanup steps:
   median 0 s, 99th percentile 14 s, maximum 42 s, 0 steps over 60 s. The finish step and the
   watchdog retry call the same `remove`. `CODE TL/finish/land.rs:907`, `CODE TL/finish/land.rs:1028`.
   ```sql
   SELECT count(*), max(s) FROM (SELECT (julianday(ended_at)-julianday(started_at))*86400 AS s
   FROM task_steps WHERE name='cleanup' AND status='passed' AND ended_at IS NOT NULL);
   ```

## How fleetd solves it

The rule: **a live process that has not finished a pass for longer than a limit of awake time is
hung, and a hung process gets SIGKILL by its exact PID and start time, then a new process starts.**

1. fleetd writes a heartbeat with an awake-clock reading after each pass. `REPORT R4`.
2. `is_hung` compares it with the current awake clock. `CODE FLEETD/domain/heartbeat.rs:45`.
3. `stop` sends SIGKILL at once to a hung process, because a polite stop ends only between 2
   passes. `CODE crates/horch/src/cmd/fleetd.rs:399` at `9bc3bf70`.
4. The awake clock is `CLOCK_UPTIME_RAW` on macOS and `CLOCK_MONOTONIC` on Linux. Neither counts
   sleep. `CODE crates/horch-platform/src/mac/awake.rs:8`, `.../linux/awake.rs:7` at `9bc3bf70`.

These 4 parts are the idea. The service manager restart, `heartbeat.json`, the generation number,
the boot id and the fleet clock exist only because fleetd is a daemon.

Do not copy the 30 s limit. `CODE FLEETD/domain/config.rs:13`. fleetd can use 30 s because each
pass has a 20 s budget and long work runs as jobs. A pass on MAIN runs `git` inline with no
timeout. `CODE CMD/taskledger_watch.rs:180`, `CODE crates/horch-marketplace/src/git.rs:158`.

## Fix

### Requirements

1. **H04-R1.** The watchdog writes a beat file, `<locks>/watchdog-<run>.lock/beat`, when it takes
   the lock. It writes the file before it records its binary, so a holder with a recorded binary
   always has a beat.
2. **H04-R2.** The watchdog writes the beat file in each pass, directly after `checkin` succeeds.
3. **H04-R3.** The beat holds the awake clock in milliseconds, as decimal text. The awake clock
   does not count machine sleep.
4. **H04-R4.** `ensure` treats a lock holder as hung when the watchdog has duties and the awake time since the beat is more than `hung_after_s(run)`. Duties are a task of the run in `queued`, `running` or `landing`, or any worktree of the ledger in `cleanup_failed`, because each pass retries all of them.
5. **H04-R5.** A missing beat, an unreadable beat, an unavailable clock, or a clock reading before
   the beat is not a hang. horch sends no signal in these cases.
6. **H04-R6.** horch stops a hung watchdog with SIGKILL, by its exact PID and recorded start time (ADR 0002), only when its process group holds that PID and no other. With no recorded start time, horch sends no signal and prints a warning.
7. **H04-R7.** After the stop, horch starts a new watchdog, prints 1 warning, and writes 1 `note`
   event whose text starts with `Watchdog hung:`.
8. **H04-R8.** `ensure` checks for a hang before it checks for a replaced binary, because the
   SIGTERM path cannot stop a hung watchdog.
9. **H04-R9.** When the group holds another PID, or horch cannot read the group, horch sends no signal and starts no watchdog. A live child can hold a git lock, so a new watchdog can hang at the same step. horch prints each child PID and writes 1 `Watchdog hung: child` event for each holder. The operator ends the child, and the pass then continues.

### Files to change

1. `crates/horch-core/src/clock.rs`: add `pub fn awake_ms() -> Option<u64>`. Use
   `libc::clock_gettime` with the 2 clocks above. Return `None` on each other platform.
2. `crates/horch-core/src/taskledger/watchdog.rs`:
   - Add `const MIN_HUNG_AFTER_S: u64 = 600`, `pub fn hung_after_s(run) -> u64`, and `const BEAT_FILE: &str = "beat"`.
   - Add `pub fn write_beat(dir, run_id) -> Result<()>`. Use `fsx::write_atomic`.
   - Add `pub beat_ms: Option<u64>` to `LockHolder`, and read it in `holder`. Add
     `beat_ms: None` to the 3 test literals at lines 1380, 1389 and 1397.
   - Add `pub fn is_hung(beat_ms: Option<u64>, now_ms: Option<u64>, limit_ms: u64) -> bool`. It is
     true only when both values exist and `now.checked_sub(beat)` is more than `limit_ms`.
   - Add `pub fn has_duties(conn, run_id)` for H04-R4. Do not change `needs_restart`.
   - Add the pure `hung_action(pid, members)`: `Kill` for exactly `[pid]`, else `Report`. `procid::group_members` (`CODE crates/horch-core/src/procid.rs:58`) returns an empty list on a read failure, and the watchdog leads its group through `setsid` (`CODE crates/horch-core/src/runtime/process.rs:214`).
   - Add `LockHolder::kill(wait)`. Copy `stop` at line 571 and send `libc::SIGKILL`. The non-Unix
     form returns an error.
3. `crates/horch/src/cmd/taskledger_watch.rs`:
   - In `take_lock`, call `write_beat` before `record_exe`. On an error, print a warning and go on.
   - In `pass`, call `write_beat` after `checkin`. On an error, print a warning and go on.
   - Add `hung(ctx, store, run, h)`: `has_duties` and `is_hung(h.beat_ms, awake_ms(), limit)`.
     The limit is the test fault `HORCH_FAULT=watchdog-hung-after-ms:<n>`
     (`crates/horch-core/src/runtime/fault.rs:32`), or else `hung_after_s(run) * 1000`.
   - In `keep_or_replace`, call `hung` first. If it is true, call `replace_hung` and return.
   - Add `replace_hung`: on `Report`, print the warning, write the H04-R9 event once, and return. On `Kill`, call `h.kill(5 s)`, call `start_detached`, print the warning, and write
     the event with `util::event` and `About { run: Some(&run.run_id), ..Default::default() }`.
     Copy the event form at `TL/runs.rs:551`. Use the binary choice at lines 431 to 436.

The warning text: `horch: warning: the watchdog of run <slug> (PID <old>) was hung; stopped it and
started PID <new>`.

### Settings

No new setting. `hung_after_s(run)` is `max(run.check_every_s, 600)`, so the default is **600 s** and the run setting of spec 05 is the 1 source of watchdog timing (`CODE TL/schema.rs:45`). A healthy watchdog
writes a beat at least every 30 s plus the time of 1 pass. `CODE CMD/taskledger_watch.rs:383`.
The 600 s floor allows 14 times the slowest measured cleanup (42 s), SQLite waits of 30 s
(`CODE TL/store.rs:9`) and reaps with a 10 s grace (`CODE CMD/taskledger.rs:1632`). A pass runs no gate (`CODE CMD/taskledger_watch.rs:117`).

### Not ported

The daemon, the service manager and `heartbeat.json` are not ported, because the orchestrator
workflow stays. The boot id is not needed, because a holder that passes the PID and start-time
check started in this boot. A pass time budget needs jobs. Windows behavior does not change,
because `awake_ms` returns `None`. A trigger for the case with no `horch` command is Q1.

### Effect at upgrade

No schema change occurs. A watchdog of an older binary has no beat file, so H04-R5 protects it.
The first watched command after the install replaces it through the existing binary check. The
`beat` file is inside the lock folder, and the lock release removes the folder
(`CODE crates/horch-core/src/fsx.rs:378`). Runs, tasks and workers do not change.

## Reproduce

Warning: do not run these steps on the live ledger. 11 fleets use it.

### Automated (NOT RUN)

The limits of this ticket forbid `cargo` and forbid a signal to a watchdog. Add this test to crate
`horch-e2e`, file `crates/horch-e2e/tests/taskledger.rs`. Copy the form of
`a_dead_watchdog_is_restarted_and_a_second_one_is_refused` at line 1351. `watched_run` gives a
running task, a live watchdog and `--check-every 1`.

```rust
#[cfg(unix)]
#[test]
fn a_live_hung_watchdog_is_stopped_and_replaced() {
    let (mut h, _teardown, _) = watched_run(world("tl-wdog-hung"));
    let run = json_of(&h, &["ledger", "status", "--json"])["run"].clone();
    let old = wait_for("the watchdog's lock and binary", || {
        lock_holder(&h, &run).filter(|l| l.exe.is_some())
    });
    let started = old.started.unwrap();
    assert!(horch_core::procid::signal_same(old.pid, started, libc::SIGSTOP));
    std::thread::sleep(std::time::Duration::from_secs(3));
    h.set("HORCH_FAULT", "watchdog-hung-after-ms:1500");
    let o = h.run(&["ledger", "status"]);
    assert!(text(&o).contains("was hung"), "{}", text(&o));
    wait_for("the hung watchdog to end", || {
        (!horch_core::procid::alive(old.pid, Some(started))).then_some(())
    });
    wait_for("the new watchdog", || {
        lock_holder(&h, &run).filter(|l| l.pid != old.pid)
    });
}
```

Expected result on MAIN: the test compiles and the `was hung` assertion fails. `_teardown` ends the
stopped process with SIGKILL. `NOT VERIFIED` until an engineer runs it.

### Manual

Use a terminal outside a fleet pane. Set `H` to the path of a `horch` built from MAIN.

- **M0 (RUN).** Find each caller of `needs_restart` in the command crate. Output:
  ```text
  crates/horch/src/cmd/taskledger_watch.rs:441:    if !watchdog::needs_restart(store.conn(), run).unwrap_or(false) {
  ```
  The only caller is `restart`, and `ensure` calls `restart` only when the lock has no holder.
- **M1 (NOT RUN).** Make a scratch ledger and a run with 1 task. Expect exit code 0.
  ```sh
  export S=$(mktemp -d) HORCH_LEDGER_DB=$S/ledger.sqlite3 HORCH_STATE_DIR=$S/state HORCH_NO_WAKE=1
  unset HORCH_NO_WATCHDOG
  git init -q -b main "$S/p" && git -C "$S/p" commit -q --allow-empty -m init && cd "$S/p"
  "$H" ledger run new h04 --check-every 1 --stall-rounds 1000
  printf '# T0\n' > t0.md && "$H" ledger add --file t0.md --primary opus
  sqlite3 "$HORCH_LEDGER_DB" "UPDATE tasks SET status='running';"
  ```
- **M2 (NOT RUN).** Run `"$H" ledger status`. Expect `the watchdog of run h04 was not running;
  started it (PID <N>)`. Write down `<N>`.
- **M3 (NOT RUN).** Run `kill -STOP <N>`, then `ps -o state= -p <N>`. Expect `T`.
- **M4 (NOT RUN).** Wait 5 s. Run `"$H" ledger status`. MAIN prints `Watchdog: horch 0.5.3, last
  checkin <old time>.` and no warning. `ps -o state= -p <N>` still prints `T`. This is the fault.
- **M5 (NOT RUN).** Clean up. Run `kill -KILL <N>`.

## Validate

Each step is `NOT RUN`, except V11. "MAIN" is the result today, before the fix.

1. **V1, H04-R7 (NOT RUN).** The test of `## Reproduce` passes. MAIN: fails at `was hung`.
2. **V2, H04-R1 (NOT RUN).** In `crates/horch-e2e/tests/watchdog_liveness.rs`, start a watchdog
   and assert `beat_ms.is_some()` when `exe.is_some()`. MAIN: does not compile.
3. **V3, H04-R2 (NOT RUN).** In the same file, set `check_every_s = 10` so the tick is 1 s, read
   the beat, and `wait_for` a larger value. MAIN: does not compile.
4. **V4, H04-R3 (NOT RUN).** In `clock.rs`, assert that 2 `awake_ms` readings 20 ms apart move
   forward on macOS and Linux. MAIN: does not compile.
5. **V5, H04-R4 (NOT RUN).** In `watchdog.rs`, assert `is_hung(Some(0), Some(600_000), 600_000)`
   is false and `is_hung(Some(0), Some(600_001), 600_000)` is true. MAIN: does not compile.
6. **V6, H04-R4 (NOT RUN).** In `watchdog_liveness.rs`, use `watchdog_world` (no task and no `cleanup_failed` row), send SIGSTOP, set the fault, run `ledger status`. Assert that the PID runs and the output has no `was hung`. MAIN: passes, so this test only guards the idle case.
7. **V7, H04-R5 (NOT RUN).** In `watchdog.rs`, assert `is_hung` is false for `(None, Some(1))`,
   `(Some(1), None)` and `(Some(5), Some(1))`. MAIN: does not compile.
8. **V8, H04-R6 (NOT RUN).** Copy `stop_ends_only_the_process_with_the_recorded_start_time` at
   `TL/watchdog.rs:1368`. Send SIGSTOP to a `sleep` child. Assert that `kill` with a wrong or no
   start time returns an error and the child runs, and that the right start time ends it. MAIN:
   does not compile.
9. **V9, H04-R8 (NOT RUN).** Copy V1 and write another binary identity to the `exe` file before
   the status command. Assert `was hung` and no `did not stop within`. MAIN: prints `did not stop
   within` and leaves the process in state `T`.
10. **V10, guard (NOT RUN).** `a_watchdog_of_this_build_is_left_alone` at
    `watchdog_liveness.rs:285` passes. MAIN: passes. It proves only that a healthy one stays.
11. **V11, baseline (RUN).** Output on 2026-11-22T23:16:33Z: `0`. MAIN writes no such event.
    ```sql
    SELECT count(*) FROM events WHERE kind='note' AND text LIKE 'Watchdog hung:%';
    ```
12. **V12, manual (NOT RUN).** Do M1 to M3, wait 5 s, and run
    `HORCH_FAULT=watchdog-hung-after-ms:1500 "$H" ledger status`. Expect `the watchdog of run h04
    (PID <N>) was hung; stopped it and started PID <M>`. `ps -p <N>` finds no process. MAIN: no
    warning, and `<N>` stays in state `T`. Clean up with `kill -KILL <M>`.
13. **V13, sleep (NOT RUN).** On a Mac, start a healthy watchdog in the scratch run with no fault.
    Run `pmset sleepnow`. Wake the machine after more than 10 minutes. Run `"$H" ledger status`.
    Expect no `was hung`. MAIN: no `was hung`, so this step only proves no false stop.
14. **V14, H04-R4 (NOT RUN).** With no open task and 1 `cleanup_failed` row, a stopped watchdog is replaced. MAIN: no `was hung`.
15. **V15, H04-R9 (NOT RUN).** `hung_action` gives `Kill` for `[p]` and `Report` for `[]` and `[p, c]`. An e2e test sets `HORCH_GIT_BIN` to a script that creates `.git/index.lock` and sleeps. Assert that the watchdog and the child still run, and the output names the child. MAIN: does not compile.
16. **V16, H04-R4 (NOT RUN).** `hung_after_s` gives 600 for `check_every_s` 1 and 600, and 1200 for 1200. MAIN: does not compile.

After 2 weeks, run the V11 query again. A count above 0 shows real hangs. Read `watchdog-<run>.log` for the step that hung.

## Size and risks

Estimate: **0.75 worker-day**, 3 production files and 9 new tests.

1. **RK1.** A healthy pass with no child over the limit gets SIGKILL. `retry_cleanup` handles all failed worktrees of the ledger in 1 pass (`CODE TL/finish/land.rs:1041`). That the next pass repairs each partial effect is `NOT VERIFIED`.
2. **RK2.** A hang inside `git` stays until the operator ends the child (H04-R9). ADR 0002 forbids a wider stop, so horch does not signal a PID that it did not record.
3. **RK3.** A restart does not remove the cause of a hang, so the watchdog can hang and restart
   every 600 s. Each restart writes 1 event, so the fault is visible.
4. **RK4.** 2 commands can see the same hang. The second start exits with `already runs`. Its
   `record_start` can overwrite `watchdog_pid` until the next checkin. `replace` has the same race.
5. **RK5.** A command in a Codex sandbox may be unable to send the signal. It then prints a
   warning, and a later command outside the sandbox does the stop. `NOT VERIFIED`.

Shared files:

1. `crates/horch-core/src/taskledger/watchdog.rs`: specs 03, 05 and 06 change `check_worker`. This spec changes `LockHolder` and `holder`.
2. `crates/horch/src/cmd/taskledger_watch.rs`: specs 03, 05 and 06. Check any work added to `pass` against `hung_after_s`.

Open questions for the operator:

1. **Q1.** Should each worker supervisor also trigger the hang check, for the case when every agent
   is idle? R6 lists this as FM-7, and no ticket of this run covers it. I recommend a separate spec.
2. **Q2.** Is a detection delay of up to 600 s acceptable? I recommend yes, because of RK1.
