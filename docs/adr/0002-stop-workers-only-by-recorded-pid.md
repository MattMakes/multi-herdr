# Stop workers only by their recorded PID and start time

A worker is stopped only by `horch ledger stop <worker>`, which signals the
exact PIDs recorded in its ledger row, each checked against its recorded
start time, and closes its exact pane. No code or briefing may stop
processes by name, pattern or transcript text (`pkill`, `killall`,
`taskkill /IM`, `kill $(pgrep ...)`), and a test fails the build if one
appears. Greedy pattern kills have taken down many workers at once in past
runs; a PID with its start time names exactly 1 process even after the OS
reuses the number.

## Consequences

- Processes an agent starts are not tracked one by one; the exact pane
  close ends them with the agent.
- Windows needs its own creation-time check before this holds there.
