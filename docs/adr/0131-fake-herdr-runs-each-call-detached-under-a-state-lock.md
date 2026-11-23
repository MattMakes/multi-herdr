# fake-herdr runs each call detached under a dead-holder-only state lock

The e2e fakes of the pane hosts (`crates/horch-e2e/src/bin/fake-herdr.rs`,
and fake-hc and fake-herdr-app through `crates/horch-e2e/src/hc_fake.rs`)
keep their state in a file guarded by a lock that the calling process
holds. A real herdr call is a request to a server, which finishes it even
when the client is killed. Here `pane close` kills a pane's whole process
group, which can hold a call of its own. So each call runs in a copy of
the fake in a new process group (`hc_fake::detach_into_own_group` for
fake-hc and fake-herdr-app), and a kill of the caller's group cannot leave
the lock stale (`<state dir>/fake-hc.lock` for fake-hc) or the state half
written.

## Consequences

- The fake-herdr `StateLock` is `$HORCH_FAKE_LOG.state.lock/`, a mkdir
  lock. The holder writes its pid into `pid` in the lock dir.
- A waiter takes the lock over only from a dead holder: one whose pid no
  longer runs, or one that left no pid after `NO_PID_GRACE` (10 s). Only a
  SIGKILL of the call itself leaves a stale lock. A live holder is never
  broken, so a slow holder under load cannot lose the state.
- Measured under load 28 to 62 (7624 calls): a hold takes at most 0.5 s
  and a wait at most 0.4 s.
- A lock dir that cannot be made at all means the harness root is gone: a
  detached `horch tile` can call herdr after the test removed its temp dir.
  The call fails then instead of waiting for ever.
- Process-group kills are queued and run only after the state is saved and
  unlocked (also `hc_fake::flush_kills`). A pane that closes itself kills
  the very process that holds the lock.
- `pane wait-output` polls without the lock. A wait polls for seconds, and
  holding the lock that long would stall the very commands whose output it
  waits for.
- A shell line entered in a pane is started and not waited for. The line
  may call herdr while this call holds the state lock. `pane wait-output`
  polls for its output.
- The OS reuses a pid under fork load. Each fake process logs a `call_id`,
  and `Harness::calls_of` merges log lines by `call_id`, not by pid. Two
  processes of one fake that got the same pid are 2 calls, not 1. A record
  without a `call_id` is keyed by its pid.
- fake-herdr follows herdr 0.8.2: there is no top-level `wait`, and
  `pane wait-output` matches pane text. A timeout is exit 1 with the
  `timeout` error code.
- horch builds PowerShell pane lines on Windows (`PaneShell::PowerShell`),
  so a fake pane runs them with PowerShell, as a real herdr pane does.
  `cmd /C` would misread the `&` call operator.
