# E2e teardown ends every process its harness started

The e2e harness teardown, `reap` in `crates/horch-e2e/src/harness.rs` with
the process helpers in `crates/horch-e2e/src/process.rs`, ends every
process a test started and returns the ones that leaked. A test that fails
before `horch done`, or never closes a pane, leaves fake-herdr `exec` pane
commands running: `horch worker` and a `stay` fake run for ever. Teardown
sends SIGTERM to the process group of every pane that fake-herdr recorded
and that is still the same group (the leader's start time matches, see
`horch_core::procid::is_same_group`). It also signals the group of every
process whose command line or environment names the harness root: a fake,
a pane whose start was never recorded, and the detached mail courier, which
redelivers until `horch inbox` acknowledges. A process of those groups, or
one that names the root, that still runs after `REAP_GRACE` (5 s) leaked.
It gets SIGKILL and is returned. A failed process listing never reads as
"no process runs": teardown would then signal nothing, report no leak and
leave a process running for hours.

## Consequences

- `process::try_processes` returns why `ps` did not run instead of an
  empty list. A fork can fail on a loaded machine, and an empty list then
  hides every process.
- The listing is tried again every 50 ms while it fails, for at most
  `REAP_GRACE`. If the processes cannot be listed within it, `reap` returns
  that error.
- A test that passes while its processes cannot be seen can leave one
  running for hours. Teardown fails it, unless it already fails.
- A sandbox can deny `ps`. Teardown then still signals the pane groups
  fake-herdr recorded and skips only the full leak scan.
- The command line and environment of each process of this user are read
  by pid, as 1 line each (`ps -E` on macOS, `/proc` on Linux). The mail
  courier names the harness only in its environment. The result is empty
  when they cannot be read.
- `process::signal_groups` sends `TERM` or `KILL` with the `kill` syscall.
  A `kill` program would need a fork, which can fail on a loaded machine,
  and then nothing is signalled. Tests that stop a process use the syscall
  too (`procid::signal_same`), for the same reason.
- A group id stays ours while the group has members: no other process can
  be given it.
- A process can start after a SIGTERM went out, even in a group that got
  it: a pane command that starts the mail courier on its way out.
- A process can leave its group between teardown's look and its SIGTERM.
  The mail courier, forked into the pane's group, calls `setsid` before it
  execs. Its pid ran at the first SIGTERM, but the signal missed it.
  Teardown signals its new group too, and the test does not fail for it.
- Each harness root holds `.horch-e2e-owner` with the owner test process's
  pid and start time. A test binary that is killed before teardown leaves
  its processes, and the detached mail courier redelivers for hours (audit
  Q3). `stop_stale_harnesses` stops the processes of every root in a parent
  dir whose owner has ended, through `reap`, which finds them by the root
  in their environment. It then removes the owner file, so each root is
  stopped once, and returns the processes that outlived SIGTERM. The dir
  stays: `HORCH_E2E_KEEP` can have kept it.
