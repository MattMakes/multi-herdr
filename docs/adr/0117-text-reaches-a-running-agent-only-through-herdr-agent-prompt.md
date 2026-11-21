# Text reaches a running agent only through `herdr agent prompt`

`horch tell` and the first prompt of a launch reach an agent by typing into
its pane (`crates/horch-core/src/messaging/delivery.rs`). Text for an agent
TUI goes through `herdr agent prompt <target> <text>` (`Herdr::agent_prompt`,
`crates/horch-core/src/workspace/herdr.rs`), never through `herdr pane run`.
`agent prompt` writes the text as 1 bracketed paste plus an encoded Enter,
atomically, even while the agent is mid-turn. It fails when herdr sees no
agent in the pane. `herdr pane run` (`Herdr::pane_run`) submits text and
Enter atomically too, which is safe for launching a command into a fresh
shell but not for a TUI in raw mode. That is why `send_line` exists for
running agents. Separate `send-text` and `send-keys enter` calls race a busy
TUI: an Enter that lands while the text is still arriving submits a head,
and the tail becomes a second, untagged message that the receiver reads as
human input. That was the truncated-`DONE` bug.

A line is typed into an agent's pane only once its agent has finished
starting and herdr reports it at its prompt
(`delivery::deliver_when_idle`, `delivery::send_line_when_ready`). Text
typed earlier is lost: OpenCode drops text typed while it starts, and a
role that registered just now has no agent in herdr yet (LA-3).

## Consequences

- herdr's `agent_status` is `idle`, `working`, `blocked`, `done` or
  `unknown` (`crates/horch-core/src/workspace/model.rs`). An agent that is
  still starting reads `unknown`, and text typed into it then is lost, so
  delivery does not type into an agent that reads `unknown`.
- `model::at_prompt` (`Pane::at_prompt`) treats both `idle` and `done` as
  waiting at the prompt. herdr reports `done` when the agent finished its
  turn and nothing has typed into the pane since, and reads `idle` again
  only after a new prompt. A Codex candidate that answered its nudge read
  `done` for 230 s and never `idle`, and a resumed agent whose last turn
  finished can read `done` (live check W3e, 2026-10-07), so a check for
  `idle` alone never saw it wait. `working`, `blocked`, `unknown` and no
  status are not at the prompt.
- `starting` treats a pane where herdr detects an agent but does not know
  its state yet (no state, or unknown) as a starting agent, and delivery
  waits. `herdr agent prompt` still succeeds then, and the text is lost
  (opencode 1.18.34, LA-3).
- When herdr detects no agent in the pane, `send_line` falls back to the
  2-call path, but presses Enter only once the end of the message shows in
  the pane, not after a fixed sleep. The second Enter is a retry that is
  safe only because the whole text has landed (Enter on an empty input does
  nothing).
- Right after a role registers, herdr shows no agent at first, then a
  starting one, then an idle one. A `horch tell` sent 3 s after spawn was
  lost while herdr showed no agent yet (LA-3). `send_line_when_ready` waits
  a grace period for a fresh role's agent to appear and become ready, and
  sends after the grace if it does not.
- The tell grace is `settings.tell_grace`, else `delivery::TELL_GRACE`,
  30 s. For that long after a role registers, `horch tell` treats a pane with no agent as one
  whose agent has not shown yet. A worker registers before its agent
  starts, and opencode took 1.6 s more to show (LA-3).
- `Readiness::DEFAULT` waits up to 120 s for a starting agent, asking herdr
  every 250 ms. An opencode resume took 5 s to reach idle on the operator's
  Mac (2026-10-04), and a loaded machine gets 24 times that.
- `deliver_when_idle` types the first prompt of a launch into its pane once
  the agent there is at its prompt, then submits it with `send_line_with`.
  It exists for a harness that drops a prompt given on its command line,
  such as an opencode resume: a resumed prompt is typed only once herdr
  reports the agent idle. When the agent is not idle within `wait.timeout`
  it returns an error: nothing was typed, and the caller says so.
- For a harness herdr does not detect itself (Prime), `Herdr::report_agent`
  reports state with `herdr pane report-agent <pane> --source <source>
  --agent <agent> --state <state> --seq <seq>` (herdr 0.8.2). `state` is
  `idle`, `working`, `blocked` or `unknown`. herdr keeps the report with the
  highest `seq` of a source, so a report with a lower one is stale.
- The pane id comes first in `report-agent`. herdr 0.8.2 answers
  `unknown option: <source>` when it comes last, as its `--help` shows it
  (live check X2, 2026-10-07).
