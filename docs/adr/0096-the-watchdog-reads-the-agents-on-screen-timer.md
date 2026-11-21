# The watchdog reads the agent's on-screen timer to see a long turn

The watchdog captures a worker pane (`herdr pane read`) and compares the
agent's on-screen "working for N" timer between checks (#31). A timer that
advanced by about the elapsed time means the agent is still in 1 long turn,
not silent. `taskledger::timer` only parses that text and does no I/O. Each
known status-line shape is 1 entry in `FORMATS`. When a CLI release changes
its wording, it gets 1 new entry and 1 fixture under
`tests/fixtures/screens/`.

## Consequences

- The timer is in seconds, or `None` when the screen shows no running timer,
  never `Some(0)` for "no timer". Only a line that also says "esc to
  interrupt" is a running timer. A finished turn ("Worked for 12m 30s") is
  not. With several timers on screen the one nearest the bottom wins.
  `Agent::Any` tries every format. A named agent tries its own formats first,
  then the others.
- Codex CLI (~0.30 and later) prints `• Working (12m 30s • esc to interrupt)`.
- Older Codex CLI (~0.1x-0.2x) prints `Working (45s · esc to interrupt)`,
  sometimes `Thinking` instead of `Working`.
- Claude Code (~1.x-2.x) prints the spinner line
  `✻ Thinking… (12m 30s · ↑ 4.2k tokens · esc to interrupt)`. The glyph and
  the verb vary on every frame.
- `timer::Format::read` lowercases a line with `to_ascii_lowercase`, which
  keeps byte offsets, so the index found for "esc to interrupt" also indexes
  the original line. Unicode lowercasing would change lengths and break the
  slicing.
- A wait has a fixed deadline. In `workers::sync_questions` the oldest open
  question of the role fixes the deadline of a question wait. Polling must
  never extend it, or a worker waiting on an unanswered question would never
  be judged.
- `workers::hold_for_finish` reads the `worker_waits` row directly, not
  through `waiting`: `waiting` ends a reviewers wait whose reviews are done,
  which they all are at that point, and would drop the deadline to keep
  (#177).
- The watchdog tests (`pass_time`) back-date the worker's last check (or
  claim) and check at that exact stamp, not at the wall clock. A check at the
  wall clock would see 1 s more whenever a second ticks after the last
  write, and a loaded machine often takes that long.
