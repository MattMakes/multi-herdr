# Remote git commands get stall limits and a wall-clock bound

`output` in `crates/horch-core/src/vcs/network.rs` is the 1 way horch runs a
git command that talks to a remote (`fetch`, `ls-remote`, `push`, `pull`).
A stalled connection must not hang a finish step: on 2026-11-08 2 finish
runs hung for more than 1 h 50 min in a `fetch` and an `ls-remote` over
ssh. `output` gives the child ssh and http stall limits and kills the
child's process group by its PID when the wall-clock bound passes. It never
retries.

## Consequences

- The limits go in as `-c` config, not as environment variables, because
  core code never reads the process environment (`arc_05`).
- Git ranks the operator's `GIT_SSH_COMMAND`, `GIT_HTTP_LOW_SPEED_LIMIT` and
  `GIT_HTTP_LOW_SPEED_TIME` above `-c`, so those still win. A key the
  operator's git config sets (for example `core.sshCommand`) is not passed.
- `stall_config` compares the operator's keys in lower case, because
  `git config --get-regexp` prints key names in lower case
  (`core.sshcommand`).
- Git starts in its own process group, so a kill also ends the ssh child
  that holds the output pipes. Otherwise the read stays blocked.
