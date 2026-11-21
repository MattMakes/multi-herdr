# Each Prime launch runs its own daemon that horch stops

Prime Agent supervises its own agents in a background daemon that outlives
the CLI. herdr already supervises: a pane is the agent's lifetime. Left
alone, `horch done` closing a worker's pane would leave that worker's Prime
daemon running, and a fleet would accumulate 1 per spawn. So in
`crates/horch-core/src/harness/prime.rs`, written for the Rust build of
Prime Agent (0.10.0), each launch gets its own launch dir,
`<state_root>/prime/<id>` (`<id>`: 12 hex digits), and its own daemon socket
in it, `<launch>/prime-agent-<uid>/d.sock`, passed as `--daemon-socket`.
`prime-agent shutdown` and `prime-agent status` ignore `--daemon-socket`:
they act on every daemon whose socket is in `$TMPDIR/prime-agent-<uid>/`.
With `TMPDIR=<launch>/` they act on this launch's daemon and nothing else,
so `Daemon::finish` stops exactly this pane's daemon and leaves the
operator's own alone. A bare `prime-agent shutdown` would stop every agent
on the machine, including other panes in the same fleet.

horch stops a Prime daemon only when it can show that the current launch
started it. The launch sweep and `finish_pane` use the scoped stop with
that launch's `TMPDIR`. A daemon of another launch is never killed.

## Consequences

- Prime launches pass only flags the installed Prime accepts. Prime 0.10.0
  refuses unknown flags ("Unknown option", exit 1). A Prime launch passes
  no `-e` status hook. The pane file names the pane instead.
- `Daemon::finish` does nothing when no process listens on the socket
  (Prime never started a daemon, or it stopped and left the socket file).
  Else it runs `TMPDIR=<launch>/ prime-agent shutdown --force --json`,
  which stops the supervisor and its workers. The stop counts only when its
  `stopped` list names this socket. The agent dir, which holds the worker
  records, is removed only after a stop.
- Without `--force` Prime refuses the shutdown ("confirmation required").
- `Daemon::shutdown` runs the stop in its own session (`spawn_detached`):
  `horch done` in a Prime pane runs under the daemon's worker, and the stop
  ends that worker. The stop must not end with it (live check X2:
  prime-6). horch waits for it, so it is not a background job.
- `STOP_WAIT` is 30 s: `prime-agent shutdown` stopped a daemon and its
  worker in about 2 s on 0.10.0, and a loaded machine is slower. Past the
  limit the stop process goes on alone and a later sweep cleans up.
- When that stop fails, `TMPDIR=<launch>/ prime-agent status --json` names
  the pid. A pid can be given to another program once its process ends, so
  `status` is asked twice. SIGTERM goes only to a pid that both answers
  name with the same start time, and that started after `Daemon::install`
  (for a sweep, after the launch's launcher file). horch never stops a
  daemon it did not start, such as the operator's own: a process that
  `status` names as started before the launcher file is left running. A
  SIGTERM to the supervisor also ends its workers within about 4 s.
  Otherwise the daemon is left running and the reason is logged. A daemon
  left running keeps its socket and its agent dir. In the test of this
  rule every child sleeps 1 h, so none ends by itself.
- `Daemon.installed` records when the launch reserved its socket. The
  daemon starts later, so a process that started earlier is not it.
  `START_SLACK` allows 1 s in that comparison: Linux counts a start time
  from a boot time in whole seconds.
- The same identity rule holds for process groups in `procid`. A group is
  the same only while its leader (pid plus start time) is, or while members
  keep it alive. After the leader ends, a child that remains keeps the
  group, and the OS cannot give out that pgid again, so the group is still
  ours.
- `pid_for_socket` rejects a pid that names no single process: 0, or one
  that turns negative as a `pid_t` (4294967295 becomes -1). A kill on 0 or
  -1 targets a process group or every process.
- `terminate` sends SIGTERM, never SIGKILL: the daemon flushes its sessions
  on the way out, and a half-written session file is a resume that fails
  later in a pane nobody is watching.
- Residual risk (U-63): Prime reports no start time of its own, and takes
  the pid from `lsof`, as a process that holds the socket. A wrong SIGTERM
  needs a process that started after this launch, got the dead daemon's
  pid and holds this launch's private socket.
- `finish_pane` stops the daemon of every launch that ran in a herdr pane
  and still has a socket (either layout), and only those, through
  `Daemon::finish` with the launcher file's time as the install time. It
  does nothing for a pane that ran no Prime launch. `horch done`
  (`crates/horch/src/cmd/messaging.rs`) calls
  `harness::prime::finish_pane` only after it closes the pane, and only
  when the close worked: herdr sends SIGHUP, then SIGTERM, then SIGKILL to
  a closed pane's processes within about 1 s, so the launcher's own finish
  often does not run (live check X2: prime-4's daemon ran on). `horch done`
  itself runs under Prime's daemon, in another session, so the close does
  not end it. A stop before the close could.
- `outlive_hangup` keeps the launcher alive when its pane closes. Closing a
  pane sends SIGHUP to the pane's foreground process group, which is horch
  and the Prime CLI. The daemon runs in its own group and does not get it.
  Before this, the default action ended horch before `Prepared::finish`,
  and the daemon ran on (a C5 daemon ran 6 h 44 min after `horch done`).
  It installs a no-op handler, not SIG_IGN: an exec resets a handler, so
  the Prime CLI still ends on the hangup, and then horch stops the daemon.
- `Daemon::finish` writes its log line with an ignored `writeln` to stderr,
  not `eprintln!`: after a hangup stderr is gone, and a failed write must
  not end the cleanup.
- Prime reports the pane's agent state to herdr itself. The Rust daemon has
  a built-in herdr reporter (`pa-daemon/src/herdr.rs`) that reads the
  pane's `HERDR_*` env and cannot be turned off. horch only releases the
  pane's agent when the pane finishes, as a clean-up after a crash. The
  release uses exactly the reporter's `--source` `herdr:pi` and `--agent`
  `prime-agent`, the labels the live check saw
  (`docs/live-checks/harnesses.md`). A release with another label does
  nothing.
- Launch dirs of Prime 0.9.4 runs have the socket at `<launch>/d.sock`. The
  launch sweep handles both layouts. Rust layout with nobody listening: the
  files are removed with no Prime call. 0.9.4 layout with nobody
  listening: the files are removed. 0.9.4 layout with a process listening:
  the Rust `prime-agent` cannot name or stop such a daemon, so horch leaves
  it running with its files. `finish_pane` handles that layout too.
- The tests make a stale socket by binding it in a child process that
  ends, so the test process never holds the socket's fd. A listener
  dropped in the test process stays alive in any process that another test
  forks before its `exec`, and a connect to it still succeeds.
