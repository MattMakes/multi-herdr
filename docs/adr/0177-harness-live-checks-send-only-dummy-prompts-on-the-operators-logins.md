# Harness live checks send only dummy prompts on the operator's logins

`scripts/live/harnesses.sh` checks the harness CLIs on the operator's Mac
(U-28, U-60..U-64, U-67, U-30 Q1): Antigravity (`agy`) flags, model,
resume and trust store, OpenCode `--prompt` on a resume, the Prime status
record and pi on the installed Node. Its result table is
`docs/live-checks/harnesses.md`, and the gate never runs it. It costs 4
small `agy` turns on the operator's Google login (a 5th is refused before
it starts) and 2 small `opencode` turns on a free tier. Both of these train
on input, so every prompt is dummy text in a scratch repo that holds 1
dummy file. These checks are never pointed at real project content.

## Consequences

- `AGY_READ_CACHE=1` also reads agy's `last_conversations.json`, which
  lists the operator's own workspaces. `AGY_TRUST_SCRATCH=1` answers agy's
  trust question for the scratch repo once, so the TUI resume step runs.
- Every harness runs through `clean`, which unsets `ANTHROPIC_API_KEY`,
  `GEMINI_API_KEY`, `GOOGLE_API_KEY` and `GOOGLE_GEMINI_BASE_URL`. The
  Anthropic key is banned in this project. Any Google key moves agy off
  the operator's Google login onto API billing.
- agy's `settings.json` is inspected by key names and a count only. Its
  values are never printed: the file can hold credentials and the
  operator's trusted workspaces.
- agy's TUI stops at a trust question in a new directory, so the first agy
  turn runs in print mode (`-p`). It passes no permission flags because the
  turn needs no tool.
- agy stores trust per path. The scratch repo is re-created at the same
  path, so after 1 `AGY_TRUST_SCRATCH=1` run the print-mode trust step
  cannot tell print mode's trust from the earlier answer, and is skipped.
- agy 1.3.0 refused a bare model id with no `--effort` before a turn
  started. agy 1.4.0 accepts it. The roster check
  (`roster/validation.rs`) still fails an agy teammate without effort, so a
  tier never depends on agy's default effort. The live step only records
  what agy does with a bare id.
- A fresh `opencode` TUI can take 1 to 5 minutes to create its session on a
  loaded Mac. The control run (the same TUI setup without `--session`,
  which delivers `--prompt`) is polled for up to 360 s and stopped at the
  control word. The resume run gets the same measured startup time plus a
  45 s window, so a slow start cannot make the "prompt ignored" check pass
  by timing.
- `scripts/live/context.sh` `statuses` samples herdr `agent_status` of a
  role's pane for 120 s, 1 per second. An OpenCode turn after a 20 s sleep
  took 66 s, so a shorter window misses the busy status.
