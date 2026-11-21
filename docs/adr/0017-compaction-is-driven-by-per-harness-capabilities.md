# Compaction is driven by per-harness capabilities

How horch asks a harness to compact, and how it tells that a compaction
failed, is data in `CompactionCaps` in
`crates/horch-core/src/harness/capabilities.rs` (CTX-13). Each harness
types and reports `/compact` differently, so the compaction job reads the
command form, the number of sends and the failure lines from the
harness's capabilities. `failure_lines` are substrings of the lines a
harness prints in its pane when a requested compaction fails. A new match
after the send stops the job.

## Consequences

- `CompactCommand::Bare` is `/compact` alone, for codex and opencode. Any
  text after it on the line becomes a new prompt to the model, so horch
  never appends compaction instructions for those harnesses.
- `max_sends` is 1 where a busy harness queues the command. Sending again
  would queue a second compaction.
- A busy rejection is not a failure line, because the job sends again.
  Codex's busy rejection, `'/compact' is disabled while a task is in
  progress.`, is left out of `CODEX_FAILURES` for this reason.
- The failure lines were gathered from each installed harness on
  2026-10-07 and must be checked again when a harness is upgraded.
- `CLAUDE_FAILURES`, from Claude Code 2.1.292 (`strings` on the binary):
  `/compact` throws `Error during compaction: ...` and
  `Compaction canceled.`, and prints `Not enough messages to compact.` as
  its output.
- `CODEX_FAILURES`, from Codex 0.160.0 (`strings` on the binary):
  `thread/compact/start failed in TUI`, the TUI error when the compact
  request fails.
- `OPENCODE_FAILURES`, from OpenCode 1.18.35 (its bundled JS):
  `Conversation history too large to compact` and
  `Session too large to compact`, the error on the summary message when
  the conversation does not fit the model.
- `PI_FAILURES`, from pi 0.99.1 (`dist/core/agent-session.js:2204`,
  `dist/modes/interactive/interactive-mode.js:2903` and `:2929`):
  `showError` prints `Error: Compaction failed: <message>` (for example
  `Nothing to compact (session too small)`) or
  `Error: Compaction cancelled`.
- `PRIME_FAILURES`, from Prime Agent 0.9.4
  (`dist/core/agent-session.js:6104`, `:6143`, `:6145`;
  `dist/modes/interactive/interactive-mode.js:4757`, `:4770`): the pi
  lines, plus 2 skip warnings that `showWarning` prints after `⚠ `
  (`Already compacted` and `Session is too short to compact`).
