# In a Claude pane the compaction writes the whats-next, and hooks save and return it

Every Claude Code pane horch launches gets 4 hooks in its `--settings`
overlay, and the watch base is 135,000 tokens (CTX-28 to CTX-31,
`docs/specs/context-policy.md` §4.6). The operator asked for both on
2026-11-21: every worker does 90% of its work in the first 150,000 tokens,
and a compaction is "precompact, run whats-next, compact, then postcompact,
hydrate with that whats-next".

The hooks are data, the `hooks` entry in
`teammates/_base/harness-defaults.md`. Each runs `horch hook <event>`:

- `PreCompact` runs `horch hook pre-compact`. It prints the `summary-format`
  line, and Claude Code uses it as instructions for the summary. The
  compaction itself then writes the whats-next: a handoff in 6 sections.
- `PostCompact` runs `horch hook post-compact`. It saves `compact_summary`
  as the session's handoff file and records `whats-next-saved`.
- `SessionStart` with matcher `compact` runs `horch hook session-start`. It
  tells the session that the summary above is its whats-next, and where it
  is saved.
- `PostToolBatch` runs `horch hook context`. At or over the threshold it
  adds the `request` line to the conversation, once per compaction cycle.

## Consequences

- The first build of this (2026-11-21) had no `PostCompact` hook and asked
  the session to write its handoff in a turn of its own before every
  compaction. The operator pointed out that `PreCompact` and `PostCompact`
  are the hooks for this. Live check part D (`docs/live-checks/context.md`,
  2026-11-22) then showed that a `PreCompact` hook's stdout shapes the
  summary exactly, so the summary can be the whats-next.
- A compaction needs no turn of the session, no turn of the orchestrator and
  no second read of the file. The summary model writes the whats-next from
  the full context at the moment of the compaction, so it is never older
  than the compaction.
- `PostCompact` saves and does not speak. Part D: after an automatic
  compaction the model does not see `PostCompact` stdout; after a manual
  `/compact` it does, inside the command output. `SessionStart` with matcher
  `compact` reaches the model after both, so the 1 line for the model comes
  from there.
- The file that `PostCompact` saves starts with a mark line.
  `horch hook session-start` does not print a marked file a second time,
  and `horch hook post-compact` replaces a marked file but never a current
  handoff file that the session wrote itself.
- `PreCompact` never blocks. Exit code 2 blocks a compaction, but the
  2.1.330 CLI counts a blocked auto-compact and stops trying after 3 in a
  row, and with the summary as the whats-next there is nothing to wait for.
- `PostToolBatch` must not exit 2: that stops the agentic loop. The line
  goes back as `hookSpecificOutput.additionalContext` with exit 0.
- The ask stays for 1 case: a pane whose native trigger is far above the
  threshold, such as the operator's own 500,000 Claude window. horch still
  does not change a native window the operator set (ADR 0016, G4). There the
  session is asked at 135,000, ends its turn, and the horch job types
  `/compact`. Where the fleet window applies, Claude Code's auto-compact
  fires first, in the middle of a turn, and nobody is asked.
- The ask reuses the `request` message and the `context-warned` event, so
  the row shows `requested` and the orchestrator does not ask a second time.
- `horch hook` always exits 0, prints nothing in a pane with no
  `HORCH_RECORD_ID`, and catches every error and panic, as the `horch note`
  check does. A broken hook must never stop a pane.
- The base moves from 300,000 to 135,000 and the fleet window of the Claude
  orchestrator from 300,000 to 200,000. A native compaction now keeps the
  task state, so a low backstop no longer loses work.
- The entry has `force`, so the 4 hooks are added even when the operator's
  settings set the same key path. Claude Code runs the hooks of every
  settings source, so the operator's own hooks still run.
- A whats-next from the summary holds what the session saw. The summary
  model cannot run `git status`, so the `hydrate-summary` line tells the
  session to run it.
- Codex, OpenCode, pi and Prime keep the protocol without hooks.
- Part D ran headless on Claude Code 2.1.330. A fleet pane in herdr has not
  run the hooks yet, and the facts must be checked again when Claude Code is
  upgraded.
