# Usage limits are read only through each harness's own client

horch reads the usage limits of every pool through the client of the
harness that draws on it, so horch never holds a provider credential (D2).
`crates/horch-core/src/routing/quota.rs` holds the pools, how full each one
is and what state that puts it in (design section 11).
`crates/horch-core/src/routing/quota_probe.rs` holds the quota probes
(design section 11.2). It is the only routing code that starts a process,
and `crate::routing::snapshot::obtain` is the only routing path that calls
it.

| pool | probe |
|---|---|
| claude | `claude -p --input-format stream-json` + one `get_usage` control request |
| codex | `codex app-server`: `initialize`, `initialized`, `account/rateLimits/read` |
| opencode-zen | none exists; a `FreeUsageLimitError` starts a cooldown |
| google | `agy -p /usage --output-format json`: a slash command, no model turn |
| local | `pi --version`, and the model in `ollama list` |

## Consequences

- Every reading is normalized on the way in. A window is named by its
  duration, never by its position. `used` is a fraction 0-1. Times are
  RFC 3339 UTC (QUO-04).
- Account ids, emails and dollar amounts never leave the parsers (TEL-11).
- opencode-zen has no quota endpoint. A `FreeUsageLimitError` refusal
  starts a cooldown instead of a reading.
- The google probe (`probe_google`) spends no quota. A slash command in agy
  print mode runs no model turn (`num_turns: 0`). It runs in `temp_root`,
  because print mode needs no trusted directory and records none
  (docs/live-checks/harnesses.md).
- `run_with_deadline` runs a probe with no stdin and a deadline. It returns
  the exit status, stdout and stderr, or `None` when the child did not exit
  in time, and the child is then killed.
- Threads read both pipes while the child runs. A child that writes more
  than the pipe buffer (64 KiB on macOS) blocks until someone reads, so
  reading only after the exit would wait for the whole deadline (U-65).
- The reader threads are not joined. A process the child started can still
  hold the pipes open after the child exits, and a join would hang.
- `probe_version_within` takes the version line from stdout or stderr.
  Some harnesses (Prime) print their `--version` output on stderr.
