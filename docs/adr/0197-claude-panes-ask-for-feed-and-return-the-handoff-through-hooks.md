# Claude panes ask for, feed and return the handoff through hooks

Every Claude Code pane horch launches gets 3 hooks in its `--settings`
overlay, and the watch base is 135,000 tokens (CTX-28 to CTX-30,
`docs/specs/context-policy.md` §4.6). The operator asked for both on
2026-11-21: every worker does 90% of its work in the first 150,000 tokens,
and a compaction is "precompact, run whats-next, compact, then postcompact,
hydrate with that whats-next".

The hooks are data, the `hooks` entry in
`teammates/_base/harness-defaults.md`. Each runs `horch hook <event>`:

- `PostToolBatch` runs `horch hook context`. At or over the threshold it
  adds the `request` line to the conversation, once per compaction cycle.
- `PreCompact` runs `horch hook pre-compact`. For a compaction with no
  argument it prints the keep-list, which Claude Code appends to the summary
  instructions.
- `SessionStart` with matcher `compact` runs `horch hook post-compact`. It
  prints the `hydrate` line and the handoff file, which Claude Code adds to
  the context.

## Consequences

- The ask is not in `PreCompact`. A `PreCompact` hook is a command and
  cannot make the model write a file, so "run whats-next" happens 1 step
  earlier, in `PostToolBatch`. The threshold is at most 0.8 of the native
  trigger, so the ask always comes before Claude Code's own auto-compact.
- The read-back is not in `PostCompact`. Claude Code shows `PostCompact`
  output to the user only. `SessionStart` with matcher `compact` is the only
  event whose output reaches the model after a compaction.
- `PreCompact` never blocks. Exit code 2 blocks a compaction, but the
  2.1.330 CLI counts a blocked auto-compact and stops trying after 3 in a
  row, and the model needs 2 requests after a block before the file can
  exist. A block could switch auto-compact off for the session.
- `PostToolBatch` must not exit 2: that stops the agentic loop. The line
  goes back as `hookSpecificOutput.additionalContext` with exit 0.
- The hook reuses the `request` message and the `context-warned` event, so
  the row shows `requested`, the orchestrator does not ask a second time,
  and the worker briefing needs no new protocol step.
- `horch hook` always exits 0, prints nothing in a pane with no
  `HORCH_RECORD_ID`, and catches every error and panic, as the `horch note`
  check does. A broken hook must never stop a pane.
- The base moves from 300,000 to 135,000 and the fleet window of the Claude
  orchestrator from 300,000 to 200,000. With the read-back hook a native
  compaction keeps the task state, so a low backstop no longer loses work.
- horch still does not change a native window the operator set (ADR 0016,
  G4). On a machine whose `~/.claude/settings.json` sets
  `CLAUDE_CODE_AUTO_COMPACT_WINDOW=500000`, a Claude pane is asked at 135,000
  and compacted by the horch job; Claude Code's own trigger stays at 467,000.
- The entry has `force`, so the 3 hooks are added even when the operator's
  settings set the same key path. Claude Code runs the hooks of every
  settings source, so the operator's own hooks still run.
- Codex, OpenCode, pi and Prime keep the protocol without hooks.
- The hook facts were read from the Claude Code hooks reference and from the
  2.1.330 binary on 2026-11-21. They must be checked again when Claude Code
  is upgraded. The hooks have not yet run in a live fleet;
  `scripts/live/context.sh` needs a step for them.
