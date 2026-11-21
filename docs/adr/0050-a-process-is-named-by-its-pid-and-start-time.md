# A process is named by its pid and start time, checked where it is stopped

A pid alone does not name a process. Once a process ends, the OS can give
its pid to any later program, so a pid read from a file, a heartbeat or a
lock can name a stranger. A signal to it ends the wrong program, and a
liveness check on it keeps a stale lock alive for ever. So
`crates/horch-core/src/procid.rs` identifies a process by its pid together
with its start time (`procid::start_time`), every stored pid is stored with
that start time, and every signal and liveness check first confirms that
the pid still has it. The check and the stop are tied together as tightly
as each OS allows, so a pid the OS has given out again is never hit. A
child the caller still holds unreaped is safe without this: its zombie
keeps the pid.

## Consequences

- The start time is opaque. It is compared for equality, never for order
  across platforms. macOS gives microseconds since the epoch
  (`proc_pidinfo`). Linux gives clock ticks since boot (`/proc/<pid>/stat`
  field 22). Windows gives the creation time from `GetProcessTimes` (a
  `FILETIME`: 100 ns ticks since 1601-01-01). Other platforms have no start
  time: `start_time` is `None` and `alive` falls back to "the pid exists".
- `os_pid` is the only way a stored pid is cast to `pid_t`. It refuses 0
  and values that would turn negative, because `kill()` with those signals
  a process group or every process.
- `signal_same` on Linux opens a pidfd first and checks the start time
  after. The pidfd names the process that had the pid when it was opened,
  and a match after that proves it is the one recorded. The signal then
  goes to that process even if it ends and its pid is given out in between.
  A kernel without pidfds (before 5.3) falls back to the check and `kill`.
- macOS has no pidfd for a process that is not a child. The start time is
  checked just before `kill`, and a race needs the pid to end and be given
  out again between the 2 calls.
- `started_by` asks whether a pid runs and started at or before a wall-clock
  time `t`. An unknown start time is not a match. On Linux it compares in
  clock ticks since boot, not with `started_at`, because `started_at` adds
  a boot time in whole seconds and can be off by up to 1 s, which could
  move a start before `t`.
- Linux counts start times from a boot time in whole seconds, so a child's
  wall-clock start can read up to 1 s plus 1 clock tick early (CI saw
  1.003 s). The `started_at` test allows 2 s of slack.
- On macOS, `imp::kinfo_proc` reads `kern.proc.pid.<pid>` (sysctl
  `KERN_PROC_PID`) for the `kinfo_proc` of a pid. Claude's macOS sandbox
  permits this query where it blocks `proc_pidinfo`, including for an
  ancestor outside the sandbox. `is_same_sandboxed` and `parent_sandboxed`
  (the parent pid) use it for that reason.
- `Stop` sets how hard a stop is. On Unix `Stop::Term` is `SIGTERM`, which
  the process can catch to clean up, and `Stop::Kill` is `SIGKILL`. Windows
  has no polite stop for an arbitrary process: both are `TerminateProcess`
  with exit code 1, and both end the process at once.
- `stop_same` stops a process only while it has its recorded start time.
  On Unix it is `signal_same` with `SIGTERM` or `SIGKILL`. On Windows it
  opens 1 handle with `PROCESS_TERMINATE` and
  `PROCESS_QUERY_LIMITED_INFORMATION`, reads the creation time through that
  handle, and calls `TerminateProcess` on the same handle only on a match.
  The handle names the process that had the pid when it was opened and
  keeps that pid from being given out, so a reused pid can never be hit
  between the check and the stop. Other platforms send nothing.
- `imp::Process::running` on Windows uses `GetExitCodeProcess` equal to
  `STILL_ACTIVE`, not handle validity. A handle can outlive its process (a
  parent holds one until it drops the child), so an open handle alone does
  not prove it runs. A process that exited with code 259 (`STILL_ACTIVE`)
  itself reads as running.
- `procid::is_same_group` holds a process group id to be the recorded
  group when its leader runs with the recorded start time and still leads
  the group, or when the leader has ended and the group still has members.
  The OS does not give the id out again while members remain.
- `heartbeat::Heartbeat` records the job's start time next to its pid. A
  pid given to another program is then not the job: not alive, not
  killed. The field is `None` in a heartbeat written before it existed.
- The e2e harness teardown and `fake-herdr`'s `pane close`
  (`crates/horch-e2e/src/process.rs`, `crates/horch-e2e/src/bin/fake-herdr.rs`)
  record a group leader's start time with its pid at `pane run` and signal
  the group only while `is_same_group` confirms it. A group without a
  recorded start time is not killed. A kill of a reused group id has ended
  another test run, even a whole gate.
- A fake's call id in the log is its pid and its start time in
  nanoseconds (`crates/horch-e2e/src/lib.rs`). The system reuses pids
  under fork load.
