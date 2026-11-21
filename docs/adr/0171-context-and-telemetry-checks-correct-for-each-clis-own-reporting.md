# Context and telemetry checks correct for each CLI's own reporting

`scripts/live/context.sh` and `scripts/live/telemetry.sh` compare horch's
numbers against what Claude Code and Codex record themselves. Each CLI
reports in its own way, so the checks correct for the known differences
instead of failing on them, and read the ledger and transcripts in a way
that survives old records and very long lines.

## Consequences

- In `claude -p` mode a Bash command with a `$` expansion needs
  interactive approval. The A3 env-merge check asks the model to run
  `printenv VAR` (allowed with `--allowedTools 'Bash(printenv:*)'`), not
  `echo "$VAR"`. The variable is unset in the process env, so only the
  settings file can supply it.
- Claude Code writes a transcript usage line only after its message
  completes, so the line's own timestamp is seconds older than its arrival
  on disk. Telemetry latency (L6) starts when the first usage line is on
  disk, polled every 0.2 s. The Claude-side lag is outside horch and is
  printed as INFO only.
- Rate limits are checked apart. Windows are matched by `window_minutes`,
  never by slot name (QUO-04). Codex 0.160 has only a weekly `primary`, a
  null `secondary` and no 5-hour window, and a rollout may hold
  `rate_limits: null`. Any snapshot with at least 1 window passes.
- Codex's own `tokens used` line leaves out the cached input, while horch's
  context number is the whole last request. The A5 codex reader check adds
  `last_token_usage.cached_input_tokens` from the rollout back before it
  compares.
- `record_of` picks a role's newest record by the newest `updated_at`, not
  by the last position in JSON order, because an old record of the same
  role stays in the ledger. It also sets `HORCH_PROJECT_DIR`, because in a
  fleet pane that variable names the pane's project, not the checked one.
- The text around the canary is extracted with `perl`, not `grep -o`,
  because `grep -o` fails on the very long JSON lines of transcripts and
  rollouts.
