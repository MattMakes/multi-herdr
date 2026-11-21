# Live checks touch only their own Prime daemon

The live checks run Prime (`prime-agent`) only as a scratch instance and
scope every status and stop call to it, so they never see or stop the
operator's own Prime daemons. `scripts/live/harnesses.sh` starts a scratch
TUI (no prompt, no model turn) on a socket in a scratch scan dir.
`scripts/live/context.sh` `stop_prime` stops only this run's daemon, the
one on `$c/d.sock`.

## Consequences

- The check scopes `status` and `shutdown` with `TMPDIR`, as the Prime
  harness does. It runs
  `TMPDIR=<dir>/ prime-agent status --json`, which must name the scratch
  TUI with pid and socketPath and no start time (U-63). The scoped
  `shutdown --force --json` must name it as stopped.
- `prime-agent model list` lists only the models Prime can serve. A
  provider it is not signed in to has none. Part C of `context.sh` runs the
  Prime steps only when the configured model is in that list.
- Step C3 (CTX-20, CTX-03) compacts pi and Prime with no argument through
  their RPC mode (`{"type":"compact"}`, sent by `rpc-compact.py`). In `-p`
  mode these harnesses read `/compact` as an ordinary prompt. After the
  compaction the marker's summary is searched for the canary, and horch
  shows the row pending, never the pre value.
